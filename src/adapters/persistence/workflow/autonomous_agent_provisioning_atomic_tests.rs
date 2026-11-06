use super::*;

async fn revision_state(f: &AdmissionFixture) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('revisions',(SELECT count(*) FROM agent_revisions AS revision JOIN agents AS agent ON agent.company_id=revision.company_id AND agent.id=revision.agent_id WHERE agent.company_id=$1 AND agent.slug='support'),'active',(SELECT count(*) FROM agent_active_revisions AS active JOIN agents AS agent ON agent.company_id=active.company_id AND agent.id=active.agent_id WHERE agent.company_id=$1 AND agent.slug='support'),'commands',(SELECT count(*) FROM workflow_provisioning_commands WHERE company_id=$1))")
        .bind(company(f).as_uuid()).fetch_one(f.persistence().pool()).await.unwrap()
}

#[tokio::test]
async fn autonomous_agent_provisioning_failed_final_command_rolls_back_initial_revision_then_competing_retries_publish_once()
 {
    let f = command_fixture().await;
    let before = written(&f).await;
    assert_eq!(
        revision_state(&f).await,
        json!({"revisions":0,"active":0,"commands":0})
    );
    sqlx::raw_sql("CREATE FUNCTION autonomous_command_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected final command commit failure'; END $$; CREATE TRIGGER autonomous_command_fault BEFORE INSERT ON workflow_provisioning_commands FOR EACH ROW EXECUTE FUNCTION autonomous_command_fault();")
        .execute(f.persistence().pool()).await.unwrap();
    assert!(
        service(&f)
            .create_assistant_channel(
                company(&f),
                owner(&f),
                command("retry-atomic", "atomic-desk")
            )
            .await
            .is_err()
    );
    assert_eq!(written(&f).await, before);
    assert_eq!(
        revision_state(&f).await,
        json!({"revisions":0,"active":0,"commands":0})
    );
    sqlx::raw_sql("DROP TRIGGER autonomous_command_fault ON workflow_provisioning_commands; DROP FUNCTION autonomous_command_fault();")
        .execute(f.persistence().pool()).await.unwrap();
    let left = service(&f);
    let right = service(&f);
    let (left, right) = tokio::join!(
        left.create_assistant_channel(
            company(&f),
            owner(&f),
            command("retry-atomic", "atomic-desk")
        ),
        right.create_assistant_channel(
            company(&f),
            owner(&f),
            command("retry-atomic", "atomic-desk")
        )
    );
    let created = left.unwrap().unwrap();
    assert_eq!(right.unwrap().unwrap(), created);
    assert_eq!(
        revision_state(&f).await,
        json!({"revisions":1,"active":1,"commands":1})
    );
    assert_eq!(
        grew(&before, &written(&f).await),
        json!({"markers":0,"receipts":0,"agents":0,"channels":1,"workflows":1,"bindings":1})
    );
    let after = written(&f).await;
    assert_eq!(
        service(&f)
            .create_assistant_channel(
                company(&f),
                owner(&f),
                command("retry-atomic", "atomic-desk")
            )
            .await
            .unwrap()
            .unwrap(),
        created
    );
    assert_eq!(written(&f).await, after);
}
