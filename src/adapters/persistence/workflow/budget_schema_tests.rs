//! Schema accounting tests deliberately use SQL; these facts grant no dispatch permission.
use super::*;

async fn limited() -> (AdmissionFixture, ActivationRequest) {
    let mut source = example("data.map");
    source["limits"]["root_budget"] = json!({"activations": 2, "model_calls": 2, "repetitions": 2});
    fixture(source).await
}

/// Category A (07-D1 §D1.2a): a root parked on one child, both under the same limits.
async fn limited_child() -> (AdmissionFixture, ActivationRequest, ActivationRequest) {
    let mut source = example("data.map");
    source["limits"]["root_budget"] = json!({"activations": 2, "model_calls": 2, "repetitions": 2});
    let (f, scopes) = AdmissionFixture::chain(source, 1).await;
    (f, scopes[0], scopes[1])
}

async fn receipt(
    db: &mut PgConnection,
    scope: ActivationRequest,
    resource: &str,
    key: &str,
    quantity: i32,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("INSERT INTO workflow_budget_receipts(company_id,run_id,execution_id,root_run_id,resource,reservation_key,quantity,disposition) SELECT $1,$2,$3,root_run_id,$4,$5,$6,'granted' FROM workflow_run_budgets WHERE company_id=$1 AND run_id=$2 ON CONFLICT DO NOTHING RETURNING disposition")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
        .bind(resource).bind(key).bind(quantity).fetch_optional(db).await
}

async fn accounting(f: &AdmissionFixture) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('identities',(SELECT jsonb_agg(to_jsonb(budget) ORDER BY root_run_id) FROM workflow_root_budgets AS budget),'usage',(SELECT jsonb_agg(to_jsonb(usage) ORDER BY root_run_id) FROM workflow_root_budget_usage AS usage),'receipts',(SELECT jsonb_agg(to_jsonb(receipt) ORDER BY run_id,execution_id,resource,reservation_key) FROM workflow_budget_receipts AS receipt))")
        .fetch_one(f.persistence().pool()).await.unwrap()
}

#[tokio::test]
async fn workflow_budget_schema_duplicate_first_claim_and_replay_with_remaining_allowance() {
    let (f, root) = limited().await;
    let barrier = tokio::sync::Barrier::new(2);
    let reserve = || async {
        let mut db = f.persistence().pool().acquire().await.unwrap();
        barrier.wait().await;
        receipt(&mut db, root, "model_call", "op", 1).await.unwrap()
    };
    let (a, b) = tokio::join!(reserve(), reserve());
    assert_ne!(a.is_some(), b.is_some());
    let before = accounting(&f).await;
    assert_eq!(before["usage"][0]["model_calls"], 1);
    assert_eq!(before["receipts"].as_array().unwrap().len(), 1);
    let mut db = f.persistence().pool().acquire().await.unwrap();
    assert!(
        receipt(&mut db, root, "model_call", "op", 1)
            .await
            .unwrap()
            .is_none()
    );
    sqlx::query("INSERT INTO workflow_budget_receipts(company_id,run_id,execution_id,root_run_id,resource,reservation_key,quantity,disposition) VALUES($1,$2,$3,$2,'model_call','op',1,'granted') ON CONFLICT(company_id,run_id,execution_id,resource,reservation_key) DO UPDATE SET quantity=EXCLUDED.quantity")
        .bind(root.company.as_uuid()).bind(root.run.as_uuid()).bind(root.execution.as_uuid()).execute(&mut *db).await.unwrap();
    assert!(receipt(&mut db, root, "model_call", "op", 2).await.is_err());
    assert_eq!(accounting(&f).await, before);
}

#[tokio::test]
async fn workflow_budget_schema_competing_descendants_and_exact_duplicate_receipts() {
    // Category B: three runs of one root reserving at once is a state the runtime never
    // produces (ancestors are parked first); it guards the shared usage-row protocol.
    let (f, root) = limited().await;
    let a = f.lineage_child(root).await;
    let b = f.lineage_child(a).await;
    for resource in ["activation", "model_call", "repetition"] {
        let key = if resource == "activation" {
            "activation"
        } else {
            "op"
        };
        let barrier = tokio::sync::Barrier::new(3);
        let reserve = |scope| {
            let f = &f;
            let barrier = &barrier;
            async move {
                let mut db = f.persistence().pool().acquire().await.unwrap();
                barrier.wait().await;
                receipt(&mut db, scope, resource, key, 1)
                    .await
                    .unwrap()
                    .unwrap()
            }
        };
        let (first, second, third) = tokio::join!(reserve(root), reserve(a), reserve(b));
        let dispositions = [first, second, third];
        assert_eq!(dispositions.iter().filter(|v| *v == "granted").count(), 2);
        assert_eq!(dispositions.iter().filter(|v| *v == "exhausted").count(), 1);
    }
    let before = accounting(&f).await;
    let barrier = tokio::sync::Barrier::new(2);
    let replay = || async {
        let mut db = f.persistence().pool().acquire().await.unwrap();
        barrier.wait().await;
        receipt(&mut db, a, "model_call", "op", 1).await.unwrap()
    };
    let (first, second) = tokio::join!(replay(), replay());
    assert!(first.is_none() && second.is_none());
    let mut db = f.persistence().pool().acquire().await.unwrap();
    assert!(receipt(&mut db, a, "model_call", "op", 2).await.is_err());
    assert_eq!(accounting(&f).await, before);
    for name in ["activations", "model_calls", "repetitions"] {
        assert_eq!(before["usage"][0][name], 2);
        assert_eq!(before["identities"][0][name], 2);
    }
}

#[tokio::test]
async fn workflow_budget_schema_rollback_scope_mutation_and_owner_cascade() {
    let (f, root, child) = limited_child().await;
    let before = accounting(&f).await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    assert_eq!(
        receipt(&mut tx, child, "model_call", "abort", 2)
            .await
            .unwrap()
            .as_deref(),
        Some("granted")
    );
    tx.rollback().await.unwrap();
    assert_eq!(accounting(&f).await, before);
    let mut db = f.persistence().pool().acquire().await.unwrap();
    receipt(&mut db, child, "model_call", "saved", 2)
        .await
        .unwrap();
    for query in [
        "UPDATE workflow_root_budgets SET model_calls=3 WHERE root_run_id=$1",
        "DELETE FROM workflow_root_budgets WHERE root_run_id=$1",
        "UPDATE workflow_root_budget_usage SET model_calls=0 WHERE root_run_id=$1",
        "UPDATE workflow_root_budget_usage SET repetitions=1 WHERE root_run_id=$1",
        "DELETE FROM workflow_root_budget_usage WHERE root_run_id=$1",
        "DELETE FROM workflow_run_budgets WHERE root_run_id=$1",
        "UPDATE workflow_run_budgets SET parent_run_id=NULL,root_run_id=run_id WHERE root_run_id=$1",
        "UPDATE workflow_budget_receipts SET quantity=1 WHERE root_run_id=$1",
        "DELETE FROM workflow_budget_receipts WHERE root_run_id=$1",
    ] {
        assert!(
            sqlx::query(query)
                .bind(root.run.as_uuid())
                .execute(&mut *db)
                .await
                .is_err(),
            "{query}"
        );
    }
    for bad in [0, -1, 1001] {
        assert!(
            receipt(&mut db, child, "model_call", "invalid", bad)
                .await
                .is_err()
        );
    }
    let foreign_execution = ActivationRequest {
        execution: root.execution,
        ..child
    };
    assert!(
        receipt(&mut db, foreign_execution, "repetition", "scope", 1)
            .await
            .is_err()
    );
    let other = f.prepare(f.request("other-root", f.manual())).await;
    f.persistence().admit(&other).await.unwrap();
    let before = accounting(&f).await;
    assert!(sqlx::query("INSERT INTO workflow_budget_receipts(company_id,run_id,execution_id,root_run_id,resource,reservation_key,quantity,disposition) VALUES($1,$2,$3,$4,'repetition','foreign',1,'granted')")
        .bind(Uuid::new_v4()).bind(child.run.as_uuid()).bind(child.execution.as_uuid()).bind(root.run.as_uuid()).execute(&mut *db).await.is_err());
    assert!(sqlx::query("INSERT INTO workflow_budget_receipts(company_id,run_id,execution_id,root_run_id,resource,reservation_key,quantity,disposition) VALUES($1,$2,$3,$4,'repetition','wrong-root',1,'granted')")
        .bind(child.company.as_uuid()).bind(child.run.as_uuid()).bind(child.execution.as_uuid()).bind(other.proposed_run_id().as_uuid()).execute(&mut *db).await.is_err());
    assert_eq!(accounting(&f).await, before);
    // A company cascade removes owners and their immutable facts together.
    sqlx::query("DELETE FROM companies WHERE id=$1")
        .bind(root.company.as_uuid())
        .execute(&mut *db)
        .await
        .unwrap();
    assert_eq!(
        accounting(&f).await,
        json!({"identities":null,"usage":null,"receipts":null})
    );
}

#[tokio::test]
async fn workflow_budget_schema_consumption_never_locks_root_run_or_execution() {
    let (f, root, child) = limited_child().await;
    let mut held = f.persistence().pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM workflow_runs WHERE id=$1 FOR UPDATE")
        .bind(root.run.as_uuid())
        .execute(&mut *held)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM workflow_executions WHERE id=$1 FOR UPDATE")
        .bind(root.execution.as_uuid())
        .execute(&mut *held)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        let mut tx = f.persistence().pool().begin().await.unwrap();
        for key in ["one", "two"] {
            assert_eq!(
                receipt(&mut tx, child, "model_call", key, 1)
                    .await
                    .unwrap()
                    .as_deref(),
                Some("granted")
            );
        }
        tx.commit().await.unwrap();
    })
    .await
    .unwrap();
    held.rollback().await.unwrap();
}

/// S-F001: the sealing branch of `insert_workflow_budget_link` and the debit rule of
/// `debit_workflow_budget_receipt`, on a fresh schema. A root run whose stored bundle is not
/// the current compiler revision, or does not decode, is `sealed_legacy` with no limits, and every positive
/// debit of it is `exhausted` without touching usage. The admitted current-revision root is
/// `frozen_v6` and grants: the control.
#[tokio::test]
async fn workflow_budget_schema_sealed_roots_exhaust_every_debit() {
    let (f, admitted) = limited().await;
    let company = admitted.company.as_uuid();
    let mut db = f.persistence().pool().acquire().await.unwrap();
    let mut sealed = Vec::new();
    for (case, bundle) in [
        (
            "older compiler revision",
            "convert_to(jsonb_set(convert_from(source.bundle,'UTF8')::jsonb,'{compiler_revision}','\"1\"')::text,'UTF8')",
        ),
        (
            "previous compiler revision",
            "convert_to(jsonb_set(convert_from(source.bundle,'UTF8')::jsonb,'{compiler_revision}','3')::text,'UTF8')",
        ),
        ("malformed bundle", "convert_to('not a bundle','UTF8')"),
    ] {
        // A raw root run beside the admitted one, as an SQL-only writer would insert it.
        let (run, execution) = (Uuid::new_v4(), Uuid::new_v4());
        sqlx::query(&format!(
            "INSERT INTO workflow_runs (company_id,id,binding_id,binding_revision,workflow_id,version_id,actor_id,trigger_id,correlation_id,bundle,input,params,resources,max_steps,max_context_bytes,deadline) \
             SELECT company_id,$2,binding_id,binding_revision,workflow_id,version_id,actor_id,gen_random_uuid(),gen_random_uuid(),{bundle},input,params,resources,max_steps,max_context_bytes,deadline \
               FROM workflow_runs AS source WHERE company_id=$1 AND id=$3"
        ))
        .bind(company)
        .bind(run)
        .bind(admitted.run.as_uuid())
        .execute(&mut *db)
        .await
        .unwrap_or_else(|error| panic!("{case}: {error}"));
        sqlx::query("INSERT INTO workflow_executions (company_id,run_id,id,step_id,activation) VALUES ($1,$2,$3,'start',1)")
            .bind(company).bind(run).bind(execution).execute(&mut *db).await
            .unwrap_or_else(|error| panic!("{case}: {error}"));
        sealed.push((
            case,
            ActivationRequest {
                company: admitted.company,
                run: RunId::new(run),
                execution: ExecutionId::new(execution),
                job: WorkflowJobId(Uuid::nil()),
            },
        ));
    }
    let budget = |run: RunId| {
        sqlx::query_scalar::<_, Value>(
            "SELECT jsonb_build_object('provenance',budget.provenance,'limits',jsonb_build_array(budget.activations,budget.model_calls,budget.repetitions),\
               'usage',jsonb_build_array(usage.activations,usage.model_calls,usage.repetitions)) \
             FROM workflow_root_budgets AS budget JOIN workflow_root_budget_usage AS usage USING (company_id,root_run_id) \
             WHERE budget.company_id=$1 AND budget.root_run_id=$2",
        )
        .bind(company)
        .bind(run.as_uuid())
        .fetch_one(f.persistence().pool())
    };
    for (case, scope) in &sealed {
        let sealed_budget =
            json!({"provenance":"sealed_legacy","limits":[null,null,null],"usage":[0,0,0]});
        assert_eq!(budget(scope.run).await.unwrap(), sealed_budget, "{case}");
        for (resource, key) in [
            ("activation", "activation"),
            ("model_call", "sealed-call"),
            ("repetition", "sealed-repeat"),
        ] {
            assert_eq!(
                receipt(&mut db, *scope, resource, key, 1)
                    .await
                    .unwrap()
                    .as_deref(),
                Some("exhausted"),
                "{case}: {resource}"
            );
        }
        assert_eq!(
            budget(scope.run).await.unwrap(),
            sealed_budget,
            "{case}: usage unchanged"
        );
    }
    // The control: the admitted current-revision root carries its frozen limits and grants.
    assert_eq!(
        budget(admitted.run).await.unwrap(),
        json!({"provenance":"frozen_v6","limits":[2,2,2],"usage":[0,0,0]})
    );
    assert_eq!(
        receipt(&mut db, admitted, "model_call", "frozen-call", 1)
            .await
            .unwrap()
            .as_deref(),
        Some("granted")
    );
    assert_eq!(
        budget(admitted.run).await.unwrap()["usage"],
        json!([0, 1, 0])
    );
}

#[tokio::test]
async fn workflow_budget_current_compiler_admission_activates() {
    use crate::application::workflow::activation::WorkflowActivation;
    let (f, scope) = limited().await;
    let activation = f.persistence().activate(scope).await.unwrap();
    assert_eq!(activation.execution, scope.execution);
    let saved: Value = sqlx::query_scalar("SELECT jsonb_build_object('compiler',(convert_from(run.bundle,'UTF8')::jsonb)->'compiler_revision','provenance',budget.provenance,'activations',usage.activations) FROM workflow_runs AS run JOIN workflow_root_budgets AS budget ON budget.company_id=run.company_id AND budget.root_run_id=run.id JOIN workflow_root_budget_usage AS usage ON usage.company_id=run.company_id AND usage.root_run_id=run.id WHERE run.company_id=$1 AND run.id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_one(f.persistence().pool()).await.unwrap();
    assert_eq!(
        saved,
        json!({"compiler":crate::application::workflow::compiler::SEMANTIC_REVISION,"provenance":"frozen_v6","activations":1})
    );
}

#[tokio::test]
async fn workflow_budget_token_policy_sql_shape_matches_compiler_contract() {
    let (f, _) = limited().await;
    let reviewer = json!({"kind":"user","id":Uuid::new_v4()});
    let valid = json!({"max_tokens":2000,"reviewers":[reviewer.clone()]});
    let mut cases = vec![
        (valid.clone(), true),
        (Value::Null, false),
        (json!({}), false),
    ];
    for amount in [
        json!(0),
        json!(-1),
        json!(1.5),
        json!(1.0),
        json!(2147483648u64),
        json!("2000"),
        Value::Null,
    ] {
        let mut policy = valid.clone();
        policy["max_tokens"] = amount;
        cases.push((policy, false));
    }
    for reviewers in [
        json!([]),
        json!([reviewer.clone(), reviewer.clone()]),
        json!([{"kind":"agent","id":Uuid::new_v4()}]),
        json!([{"kind":"user","id":"bad"}]),
        json!([{"kind":"user"}]),
        json!([{"kind":"user","id":Uuid::new_v4(),"extra":true}]),
    ] {
        let mut policy = valid.clone();
        policy["reviewers"] = reviewers;
        cases.push((policy, false));
    }
    let mut unknown = valid.clone();
    unknown["money"] = json!(10);
    cases.push((unknown, false));
    let mut maximum = valid;
    maximum["max_tokens"] = json!(2147483647);
    cases.push((maximum, true));
    for (policy, expected) in cases {
        let accepted: bool = sqlx::query_scalar("SELECT valid_workflow_token_policy($1)")
            .bind(&policy)
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
        assert_eq!(accepted, expected, "{policy}");
    }
}
