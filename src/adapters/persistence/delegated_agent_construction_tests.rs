use super::*;

#[derive(Debug, PartialEq, sqlx::FromRow)]
pub(super) struct Counts {
    pub(super) agents: i64,
    pub(super) principals: i64,
    pub(super) revisions: i64,
    pub(super) workflows: i64,
    pub(super) bindings: i64,
    pub(super) members: i64,
    pub(super) grants: i64,
}

pub(super) async fn counts(m: &ManagedAgents) -> Counts {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM agents WHERE company_id=$1) AS agents, \
        (SELECT count(*) FROM principals WHERE company_id=$1) AS principals, \
        (SELECT count(*) FROM agent_revisions WHERE company_id=$1) AS revisions, \
        (SELECT count(*) FROM workflow_definitions WHERE company_id=$1) AS workflows, \
        (SELECT count(*) FROM workflow_bindings WHERE company_id=$1) AS bindings, \
        (SELECT count(*) FROM department_members WHERE company_id=$1) AS members, \
        (SELECT count(*) FROM channel_principal_grants WHERE company_id=$1) AS grants",
    )
    .bind(m.d.f.company)
    .fetch_one(m.p().pool())
    .await
    .unwrap()
}

#[tokio::test]
async fn delegated_admin_agent_creation_rolls_back_early_membership_and_publications_on_final_grant_failure()
 {
    let Some(m) = ManagedAgents::new().await else {
        return;
    };
    let before = counts(&m).await;
    sqlx::raw_sql(r#"
        CREATE FUNCTION fail_delegated_agent_grant() RETURNS trigger LANGUAGE plpgsql AS $$
        DECLARE selected uuid;
        BEGIN
            SELECT agent.id INTO selected FROM agents AS agent JOIN principals AS principal
                ON principal.company_id=agent.company_id AND principal.agent_id=agent.id
                WHERE agent.company_id=NEW.company_id AND agent.slug='rollback-target'
                    AND principal.id=NEW.principal_id;
            IF selected IS NOT NULL THEN
                IF NOT EXISTS(SELECT 1 FROM agent_revisions WHERE company_id=NEW.company_id AND agent_id=selected)
                    OR NOT EXISTS(SELECT 1 FROM workflow_definitions WHERE company_id=NEW.company_id AND owner_agent_id=selected) THEN
                    RAISE EXCEPTION 'fixture grant arrived before publication and native construction';
                END IF;
                RAISE EXCEPTION 'fixture final grant failure';
            END IF;
            RETURN NEW;
        END $$;
        CREATE TRIGGER fail_delegated_agent_grant BEFORE INSERT ON channel_principal_grants
            FOR EACH ROW EXECUTE FUNCTION fail_delegated_agent_grant();
    "#).execute(m.p().pool()).await.unwrap();
    let error = m
        .service
        .create(
            m.actor(),
            m.d.company(),
            write("rollback-target"),
            Some(AgentPlacement {
                department: m.d.unit.id,
                role: None,
            }),
        )
        .await
        .unwrap_err();
    assert!(
        format!("{error:?}").contains("fixture final grant failure"),
        "{error:?}"
    );
    assert_eq!(counts(&m).await, before);
    assert!(
        m.p()
            .list_by_company_id(m.d.f.company)
            .await
            .unwrap()
            .iter()
            .all(|agent| agent.slug.as_str() != "rollback-target")
    );
}
