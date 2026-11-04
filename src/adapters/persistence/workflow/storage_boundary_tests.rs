use super::*;

struct BoundaryTemplate {
    run: Value,
    wait: Value,
    source: Value,
}
async fn add_members(
    f: &AdmissionFixture,
    root: ActivationRequest,
    template: &BoundaryTemplate,
    offset: i64,
    count: i64,
) {
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlx::query("CREATE TEMP TABLE boundary_members(execution_id uuid,run_id uuid,wait_id uuid,step_id text,activation integer) ON COMMIT DROP")
        .execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO pg_temp.boundary_members SELECT gen_random_uuid(),gen_random_uuid(),gen_random_uuid(),'boundary-'||n::text,n::integer+1 FROM generate_series($1::bigint+1,$1+$2) AS n")
        .bind(offset).bind(count).execute(&mut *tx).await.unwrap();
    sqlx::query(
        "INSERT INTO workflow_executions(company_id,run_id,id,step_id,activation)
        SELECT $1,$2,execution_id,step_id,activation FROM pg_temp.boundary_members",
    )
    .bind(root.company.as_uuid())
    .bind(root.run.as_uuid())
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("INSERT INTO workflow_runs SELECT (jsonb_populate_record(NULL::workflow_runs,
        $1::jsonb||jsonb_build_object('id',member.run_id,'parent_execution_id',member.execution_id,'trigger_id',gen_random_uuid()))).*
        FROM pg_temp.boundary_members AS member")
        .bind(&template.run).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO workflow_admissions(company_id,binding_id,source_key,run_id)
        SELECT $1,($2::jsonb->>'binding_id')::uuid,'v1:'||replace(jsonb_set(jsonb_set($3::jsonb,'{2}',to_jsonb(member.execution_id::text)),
            '{3}',to_jsonb(member.step_id))::text,' ',''),member.run_id FROM pg_temp.boundary_members AS member")
        .bind(root.company.as_uuid()).bind(&template.run).bind(&template.source).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO workflow_waits SELECT (jsonb_populate_record(NULL::workflow_waits,
        $1::jsonb||jsonb_build_object('id',member.wait_id,'execution_id',member.execution_id,'child_run_id',member.run_id,
            'state','cancelled','settled_at',clock_timestamp()))).* FROM pg_temp.boundary_members AS member")
        .bind(&template.wait).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn run_retention_native_schema_100001_members_reaches_delete_without_activation_cutoff() {
    let source = serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
    let (f, scopes) = Box::pin(AdmissionFixture::chain(source, 1)).await;
    let (root, child) = (scopes[0], scopes[1]);
    let (run,wait,source):(Value,Value,Value)=sqlx::query_as("SELECT to_jsonb(run),to_jsonb(wait),substring(admission.source_key FROM 4)::jsonb
        FROM workflow_runs AS run JOIN workflow_waits AS wait ON wait.company_id=run.company_id AND wait.child_run_id=run.id
        JOIN workflow_admissions AS admission ON admission.company_id=run.company_id AND admission.run_id=run.id WHERE run.id=$1")
        .bind(child.run.as_uuid()).fetch_one(f.persistence().pool()).await.unwrap();
    let template = BoundaryTemplate { run, wait, source };
    // One genuinely admitted child closes without consuming an activation.
    old(&f, child).await;
    // Native schema scaling, not100001 runtime admissions. Every synthetic child has
    // a distinct parent execution, matching admission and closed parent wait.
    let mut offset = 0;
    while offset < 99999 {
        let count = (99999 - offset).min(1000);
        add_members(&f, root, &template, offset, count).await;
        offset += count;
    }
    sqlx::query("UPDATE workflow_runs SET state='cancelled',waiting_reason=NULL WHERE parent_run_id=$1 AND state='queued'")
        .bind(root.run.as_uuid()).execute(f.persistence().pool()).await.unwrap();
    usage(&f, root, None).await;
    run_finished_at_tests::drive(
        &f,
        root.run,
        crate::application::workflow::reads::RunStateName::Cancelled,
    )
    .await;
    let candidate = age(&f, root).await;
    assert_eq!(count(&f, "workflow_runs").await, 100001);
    sqlx::query("ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable")
        .execute(f.persistence().pool())
        .await
        .unwrap();
    sqlx::raw_sql("ANALYZE workflow_runs,workflow_executions,workflow_run_budgets,workflow_waits,workflow_admissions")
        .execute(f.persistence().pool()).await.unwrap();
    // The exact first-DELETE marker proves ownership/capture/eligibility passed the
    // former cutoff. It stops before expensive cascades; no purge latency is claimed.
    sqlx::raw_sql("CREATE FUNCTION retention_boundary_marker() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN RAISE EXCEPTION 'retention boundary reached delete'; END $$;
        CREATE TRIGGER retention_boundary_marker BEFORE DELETE ON workflow_runs FOR EACH ROW EXECUTE FUNCTION retention_boundary_marker()")
        .execute(f.persistence().pool()).await.unwrap();
    let error = f.persistence().retire_tree(candidate).await.unwrap_err();
    assert!(
        format!("{error:?}").contains("retention boundary reached delete"),
        "{error:?}"
    );
    assert_eq!(count(&f, "workflow_runs").await, 100001);
    assert_eq!(count(&f, "background_tasks").await, 2);
    assert_eq!(count(&f, "model_usage_calls").await, 1);
    assert_eq!(count(&f, "model_usage_history").await, 0);
}
