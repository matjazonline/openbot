use super::*;

#[tokio::test]
async fn autonomous_agent_storage_manifest_seals_selection_and_accepts_only_its_tree() {
    let (f, scopes) = super::native_runs::unsealed_chain().await;
    let agent = new_agent(&f, Vec::new()).await;
    let revision = save_revision(&f, &definition(&f, agent.id, Vec::new())).await;
    let value = manifest_for(&f, scopes[0].run, agent.id, revision).await;
    let manifest = save_manifest(&f, &value).await;
    let foreign = f.foreign().await;
    let foreign_agent = new_agent(&foreign, Vec::new()).await;
    let foreign_revision = save_revision(
        &foreign,
        &definition(&foreign, foreign_agent.id, Vec::new()),
    )
    .await;
    native_rejection(sqlx::query("INSERT INTO agent_manifest_revisions(company_id,manifest_id,agent_id,revision_id) VALUES($1,$2,$3,$4)")
        .bind(f.binding.target.company.as_uuid()).bind(manifest).bind(foreign_agent.id).bind(foreign_revision)
        .execute(f.persistence().pool()).await.unwrap_err());
    attach_run(&f, scopes[1].run, scopes[0].run, manifest)
        .await
        .unwrap();
    let other = super::native_runs::unsealed_root(&f, "wrong-tree").await;
    native_rejection(
        attach_run(&f, other.run, scopes[0].run, manifest)
            .await
            .unwrap_err(),
    );
    let extra = new_agent(&f, Vec::new()).await;
    let extra_revision = save_revision(&f, &definition(&f, extra.id, Vec::new())).await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlx::query("INSERT INTO agent_manifest_revisions(company_id,manifest_id,agent_id,revision_id) VALUES($1,$2,$3,$4)")
        .bind(f.binding.target.company.as_uuid()).bind(manifest).bind(extra.id).bind(extra_revision).execute(&mut *tx).await.unwrap();
    native_rejection(tx.commit().await.unwrap_err());
    let workflow = WorkflowId::new(Uuid::new_v4());
    let version = f
        .binding
        .fixture
        .publish_workflow(
            workflow,
            serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap(),
        )
        .await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlx::query("INSERT INTO agent_manifest_workflows(company_id,manifest_id,workflow_id,version_id) VALUES($1,$2,$3,$4)")
        .bind(f.binding.target.company.as_uuid()).bind(manifest).bind(workflow.as_uuid()).bind(version.as_uuid()).execute(&mut *tx).await.unwrap();
    native_rejection(tx.commit().await.unwrap_err());
    for change in [
        "DELETE FROM agent_dependency_manifests WHERE id=$1",
        "UPDATE agent_dependency_manifests SET content=content WHERE id=$1",
    ] {
        native_rejection(
            sqlx::query(change)
                .bind(manifest)
                .execute(f.persistence().pool())
                .await
                .unwrap_err(),
        );
    }
    native_rejection(
        sqlx::query("DELETE FROM run_dependency_manifests WHERE run_id=$1")
            .bind(scopes[1].run.as_uuid())
            .execute(f.persistence().pool())
            .await
            .unwrap_err(),
    );
}

#[tokio::test]
async fn autonomous_agent_storage_manifest_retention_races_preserve_surviving_revisions() {
    let (f, scopes) = super::native_runs::unsealed_chain().await;
    let agent = new_agent(&f, Vec::new()).await;
    let revision = save_revision(&f, &definition(&f, agent.id, Vec::new())).await;
    let manifest = save_manifest(
        &f,
        &manifest_for(&f, scopes[0].run, agent.id, revision).await,
    )
    .await;
    attach_run(&f, scopes[1].run, scopes[0].run, manifest)
        .await
        .unwrap();
    usage(&f, scopes[1], Some("reported")).await;
    let survivor = super::native_runs::unsealed_root(&f, "surviving-root").await;
    let survivor_manifest = save_manifest(
        &f,
        &manifest_for(&f, survivor.run, agent.id, revision).await,
    )
    .await;
    let live_root = RetentionRoot {
        company: survivor.company,
        run: survivor.run,
        finished_at: Utc::now() - Duration::days(91),
    };
    assert_eq!(f.persistence().retire_tree(live_root).await.unwrap(), None);
    let root = old(&f, scopes[0]).await;
    let (left, right) = tokio::join!(
        f.persistence().retire_tree(root),
        f.persistence().retire_tree(root)
    );
    assert_eq!(
        usize::from(left.unwrap().is_some()) + usize::from(right.unwrap().is_some()),
        1
    );
    assert_eq!(count(&f, "agent_dependency_manifests").await, 1);
    let remaining: Uuid = sqlx::query_scalar("SELECT id FROM agent_dependency_manifests")
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(remaining, survivor_manifest);
    assert_eq!(count(&f, "agent_revisions").await, 1);
    assert_eq!(count(&f, "model_usage_history").await, 1);
    assert_eq!(count(&f, "run_dependency_manifests").await, 1);
    sqlx::query("DELETE FROM companies WHERE id=$1")
        .bind(f.binding.target.company.as_uuid())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(count(&f, "agent_dependency_manifests").await, 0);
    assert_eq!(count(&f, "agent_revisions").await, 0);
}
