use super::*;
use crate::application::agent::{
    AgentPublicationKey, AgentPublicationScope, AgentPublications, PublishAgent,
};
use crate::application::workflow::agent::AgentPreparation;
use crate::entities::agent_capability::AgentId;

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
struct Selection {
    manifest: Uuid,
    agent: Uuid,
    revision: Uuid,
    version: Uuid,
}

async fn selected(setup: &AgentSetup, run: RunId) -> Selection {
    sqlx::query_as("SELECT ownership.manifest_id AS manifest,selection.agent_id AS agent,selection.revision_id AS revision,run.version_id AS version \
        FROM workflow_runs AS run JOIN run_dependency_manifests AS ownership ON ownership.company_id=run.company_id AND ownership.run_id=run.id \
        JOIN agent_manifest_revisions AS selection ON selection.company_id=ownership.company_id AND selection.manifest_id=ownership.manifest_id \
        WHERE run.company_id=$1 AND run.id=$2")
        .bind(setup.fixture.target.company.as_uuid()).bind(run.as_uuid()).fetch_one(setup.fixture.persistence.pool()).await.unwrap()
}

fn scope(setup: &AgentSetup) -> AgentPublicationScope {
    AgentPublicationScope {
        company: setup.fixture.target.company,
        actor: setup.fixture.target.actor,
        agent: AgentId::new(setup.agent),
    }
}

pub(super) async fn request(setup: &AgentSetup, key: &str, trigger: Uuid) -> AdmitWorkflowRequest {
    let company = setup.fixture.target.company;
    let binding: Uuid =
        sqlx::query_scalar("SELECT binding_id FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(company.as_uuid())
            .bind(setup.claim.fence.scope.run.as_uuid())
            .fetch_one(setup.fixture.persistence.pool())
            .await
            .unwrap();
    AdmitWorkflowRequest {
        history_session: None,
        company_id: company,
        actor: setup.fixture.target.actor,
        association: RelatedAssociation::Company,
        trigger: TriggerRef::new(company, TriggerId::new(trigger), TriggerSource::Manual).unwrap(),
        correlation_id: CorrelationId::new(),
        binding_id: WorkflowBindingId::new(binding),
        idempotency_key: IdempotencyKey::parse(key).unwrap(),
        input: json!({}),
        rerun_of: None,
        delivery_policy: crate::application::workflow::DeliveryPolicy::Deliver,
        expected_binding_revision: None,
        admission_claim: None,
    }
}

fn created(result: AdmissionResult) -> RunId {
    match result {
        AdmissionResult::Created(run) => run,
        other => panic!("expected fresh run, got {other:?}"),
    }
}

#[tokio::test]
async fn autonomous_agent_admission_revisions_are_selected_at_admission_and_replay_before_activation()
 {
    let (origin, server) = server(HTTP_OK).await;
    let setup = Box::pin(agent_setup(&origin)).await;
    let p = &setup.fixture.persistence;
    let service = WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight);
    let queued = created(
        service
            .admit(request(&setup, "queued-first-revision", Uuid::new_v4()).await)
            .await
            .unwrap(),
    );
    let old = selected(&setup, queued).await;
    let state: String =
        sqlx::query_scalar("SELECT state FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(scope(&setup).company.as_uuid())
            .bind(queued.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(state, "queued");
    sqlx::query(
        "UPDATE agents SET system_prompt='Published second behavior' WHERE company_id=$1 AND id=$2",
    )
    .bind(scope(&setup).company.as_uuid())
    .bind(setup.agent)
    .execute(p.pool())
    .await
    .unwrap();
    let published = setup.fixture.publish_agent(setup.agent).await;
    let trigger = Uuid::new_v4();
    let run = created(
        service
            .admit(request(&setup, "new-revision", trigger).await)
            .await
            .unwrap(),
    );
    let new = selected(&setup, run).await;
    assert_ne!(new.manifest, old.manifest);
    assert_ne!(new.revision, old.revision);
    assert_eq!(new.revision, published.id.as_uuid());
    assert_eq!(
        new.version, old.version,
        "publishing an agent leaves the workflow version intact"
    );
    let launch = p
        .prepare_agent(
            &setup.claim,
            crate::application::workflow::lease::LeasePolicy::new(std::time::Duration::from_secs(
                60,
            ))
            .unwrap(),
        )
        .await
        .unwrap()
        .expect_ready();
    assert!(
        !launch
            .prepared
            .preamble()
            .contains("Published second behavior")
    );
    assert_eq!(
        launch.prepared.selection().revision().id.as_uuid(),
        old.revision
    );
    AgentPublications::deactivate_agent(p, scope(&setup), Some(published.id))
        .await
        .unwrap();
    let replay = service
        .admit(request(&setup, "new-revision", trigger).await)
        .await
        .unwrap();
    assert!(matches!(replay, AdmissionResult::Replayed(id) if id == run));
    assert_eq!(selected(&setup, run).await, new);
    let before = run_count(&setup).await;
    assert!(
        service
            .admit(request(&setup, "inactive-new-root", Uuid::new_v4()).await)
            .await
            .is_err()
    );
    let after = run_count(&setup).await;
    assert_eq!(
        before, after,
        "missing active selection rolls back the entire new admission"
    );
    drop(server);
}

async fn run_count(setup: &AgentSetup) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM workflow_runs WHERE company_id=$1")
        .bind(scope(setup).company.as_uuid())
        .fetch_one(setup.fixture.persistence.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn autonomous_agent_admission_competing_duplicates_and_publication_share_one_complete_selection()
 {
    let (origin, server) = server(HTTP_OK).await;
    let setup = Box::pin(agent_setup(&origin)).await;
    let p = &setup.fixture.persistence;
    let old = selected(&setup, setup.claim.fence.scope.run).await;
    sqlx::query(
        "UPDATE agents SET system_prompt='Racing publication' WHERE company_id=$1 AND id=$2",
    )
    .bind(scope(&setup).company.as_uuid())
    .bind(setup.agent)
    .execute(p.pool())
    .await
    .unwrap();
    let preview = AgentPublications::preview_agent(p, scope(&setup))
        .await
        .unwrap();
    let publication = PublishAgent {
        scope: scope(&setup),
        expected_draft_revision: preview.draft_revision,
        expected_active_revision: preview.active_revision,
        key: AgentPublicationKey::parse("competing-admission").unwrap(),
    };
    let service = WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight);
    let trigger = Uuid::new_v4();
    let left = request(&setup, "duplicate-root", trigger).await;
    let right = request(&setup, "duplicate-root", trigger).await;
    let (left, right, publication) = tokio::join!(
        service.admit(left),
        service.admit(right),
        AgentPublications::publish_agent(p, publication)
    );
    let published = publication.unwrap();
    let run = match (left.unwrap(), right.unwrap()) {
        (AdmissionResult::Created(left), AdmissionResult::Replayed(right))
        | (AdmissionResult::Replayed(left), AdmissionResult::Created(right)) => {
            assert_eq!(left, right);
            left
        }
        other => panic!("expected one root and one replay, got {other:?}"),
    };
    let winner = selected(&setup, run).await;
    assert!(winner.revision == old.revision || winner.revision == published.id.as_uuid());
    assert_eq!(winner.agent, setup.agent);
    #[derive(sqlx::FromRow)]
    struct ManifestCounts {
        attachments: i64,
        selections: i64,
    }
    let counts: ManifestCounts = sqlx::query_as("SELECT (SELECT count(*) FROM run_dependency_manifests WHERE company_id=$1 AND run_id=$2) AS attachments,(SELECT count(*) FROM agent_manifest_revisions WHERE company_id=$1 AND manifest_id=$3) AS selections")
        .bind(scope(&setup).company.as_uuid()).bind(run.as_uuid()).bind(winner.manifest).fetch_one(p.pool()).await.unwrap();
    assert_eq!(counts.attachments, 1);
    assert_eq!(counts.selections, 1);
    let next = created(
        service
            .admit(request(&setup, "after-publication", Uuid::new_v4()).await)
            .await
            .unwrap(),
    );
    assert_eq!(
        selected(&setup, next).await.revision,
        published.id.as_uuid()
    );
    drop(server);
}

#[tokio::test]
async fn autonomous_agent_workflow_reference_rename_cannot_redirect_an_existing_workflow() {
    let (origin, server) = server(HTTP_OK).await;
    let setup = Box::pin(agent_setup(&origin)).await;
    let p = &setup.fixture.persistence;
    let old = selected(&setup, setup.claim.fence.scope.run).await;
    crate::adapters::persistence::test_support::rename_native_agent(
        p,
        scope(&setup),
        "renamed-assistant",
    )
    .await;
    let original = setup.fixture.publish_agent(setup.agent).await;
    let replacement = AgentPersistence::create(
        p,
        scope(&setup).company.as_uuid(),
        AgentWrite {
            name: "Replacement".into(),
            slug: "assistant".into(),
            provider: Some(SCRIPTED_PROVIDER.into()),
            model: Some(SCRIPTED_MODEL.into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    setup.fixture.publish_agent(replacement.id).await;
    let service = WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight);
    let run = created(
        service
            .admit(request(&setup, "after-rename", Uuid::new_v4()).await)
            .await
            .unwrap(),
    );
    let selection = selected(&setup, run).await;
    assert_eq!(selection.agent, setup.agent);
    assert_ne!(selection.agent, replacement.id);
    assert_eq!(selection.revision, original.id.as_uuid());
    assert_eq!(selection.version, old.version);
    drop(server);
}

#[tokio::test]
async fn autonomous_agent_authority_checkpoint_requires_the_exact_selected_agent_identity() {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let p = &setup.fixture.persistence;
    let launch = crate::application::workflow::agent::AgentPreparation::prepare_agent(
        p,
        &setup.claim,
        crate::application::workflow::lease::LeasePolicy::new(std::time::Duration::from_secs(60))
            .unwrap(),
    )
    .await
    .unwrap()
    .expect_ready();
    let mut prepared = launch.prepared.spec(setup.claim.fence).unwrap().prepared;
    let scope = crate::application::workflow::actions::ActionScope {
        company: setup.claim.fence.scope.company,
        run: setup.claim.fence.scope.run,
        execution: setup.claim.fence.scope.execution,
    };
    let mut db = p.pool().acquire().await.unwrap();
    assert!(
        crate::adapters::persistence::workflow::agent_preparation::current_on(
            &mut db, scope, &prepared
        )
        .await
        .is_ok()
    );
    prepared["selection"]["agent"] = json!(Uuid::new_v4());
    assert!(
        crate::adapters::persistence::workflow::agent_preparation::current_on(
            &mut db, scope, &prepared
        )
        .await
        .is_err()
    );
    let calls: i64 = sqlx::query_scalar("SELECT count(*) FROM model_usage_calls WHERE run_id=$1")
        .bind(scope.run.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(calls, 0);
}

fn outside(error: &AppError, code: &str, resource: Uuid) -> bool {
    matches!(error, AppError::ResourceOutsideUnit(refusal)
        if refusal.code() == code && refusal.resource == resource)
}

async fn agent_skill(setup: &AgentSetup) -> Uuid {
    sqlx::query_scalar("SELECT skill_id FROM agent_skills WHERE agent_id=$1")
        .bind(setup.agent)
        .fetch_one(setup.fixture.persistence.pool())
        .await
        .unwrap()
}

/// Place the agent's principal in `unit` (a role there is a placement, Plan07 decision d).
async fn place_agent(setup: &AgentSetup, unit: Uuid) {
    sqlx::query(
        "INSERT INTO department_roles (company_id, department_id, role, principal_id) \
         SELECT $1, $2, 'lead', id FROM principals WHERE company_id=$1 AND agent_id=$3 \
         AND kind='agent'",
    )
    .bind(setup.fixture.target.company.as_uuid())
    .bind(unit)
    .bind(setup.agent)
    .execute(setup.fixture.persistence.pool())
    .await
    .unwrap();
}

/// Plan07 D9b with decision d: admission checks the agent's skills and tool connections against
/// their current units and the agent's current placements; a resource moved up stays usable.
#[tokio::test]
async fn unit_resource_selection_admission_agent_uses_current_units_and_placement() {
    let (origin, _server) = server(HTTP_OK).await;
    let setup = Box::pin(agent_setup_with(
        &origin,
        Some("Follow the support policy."),
    ))
    .await;
    let p = &setup.fixture.persistence;
    let company = setup.fixture.target.company;
    let owner = setup.fixture.target.actor.user_id();
    let service = WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight);
    let skill = agent_skill(&setup).await;
    let root = p.test_root_unit(company.as_uuid()).await;
    let billing = p.test_unit(company.as_uuid(), "billing").await;
    let support = p.test_unit(company.as_uuid(), "support").await;
    let admit = |key: &'static str| {
        let service = &service;
        let setup = &setup;
        async move {
            service
                .admit(request(setup, key, Uuid::new_v4()).await)
                .await
        }
    };
    created(admit("unit-baseline").await.unwrap());
    let runs = run_count(&setup).await;
    // Moved into Billing, the unplaced agent (the root's reach) can no longer use it.
    crate::use_cases::skill::SkillManagementPersistence::move_company(
        p,
        owner,
        company.as_uuid(),
        skill,
        billing,
    )
    .await
    .unwrap();
    let refused = admit("unit-skill-moved").await;
    assert!(
        refused.as_ref().err().is_some_and(|error| outside(
            error,
            "admission.resource_outside_unit",
            skill
        )),
        "{:?}",
        refused.err()
    );
    assert_eq!(run_count(&setup).await, runs, "refused before any run row");
    // Placed in Billing, the agent reaches it without republishing.
    place_agent(&setup, billing).await;
    created(admit("unit-agent-placed").await.unwrap());
    // Moved up to the root, it stays in reach.
    crate::use_cases::skill::SkillManagementPersistence::move_company(
        p,
        owner,
        company.as_uuid(),
        skill,
        root,
    )
    .await
    .unwrap();
    created(admit("unit-skill-up").await.unwrap());
    // The tool's connection moved to the sibling Support is out of the agent's reach.
    p.move_http(
        company,
        setup.fixture.target.actor,
        setup.connection.id,
        crate::entities::department::DepartmentId::new(support),
    )
    .await
    .unwrap();
    let refused = admit("unit-connection-moved").await;
    assert!(
        refused.as_ref().err().is_some_and(|error| outside(
            error,
            "admission.resource_outside_unit",
            setup.connection.id
        )),
        "{:?}",
        refused.err()
    );
}

/// Plan07 01B.2: publishing an agent refuses a skill or connection outside its reach.
#[tokio::test]
async fn unit_resource_selection_publication_agent_refuses_out_of_reach_resources() {
    let (origin, _server) = server(HTTP_OK).await;
    let setup = Box::pin(agent_setup_with(
        &origin,
        Some("Follow the support policy."),
    ))
    .await;
    let p = &setup.fixture.persistence;
    let company = setup.fixture.target.company;
    let owner = setup.fixture.target.actor.user_id();
    let skill = agent_skill(&setup).await;
    let root = p.test_root_unit(company.as_uuid()).await;
    let billing = p.test_unit(company.as_uuid(), "billing").await;
    use crate::use_cases::skill::SkillManagementPersistence;
    SkillManagementPersistence::move_company(p, owner, company.as_uuid(), skill, billing)
        .await
        .unwrap();
    let refused = setup.fixture.try_publish_agent(setup.agent).await;
    assert!(
        refused.as_ref().err().is_some_and(|error| outside(
            error,
            "publication.resource_outside_unit",
            skill
        )),
        "{:?}",
        refused.err()
    );
    SkillManagementPersistence::move_company(p, owner, company.as_uuid(), skill, root)
        .await
        .unwrap();
    p.move_http(
        company,
        setup.fixture.target.actor,
        setup.connection.id,
        crate::entities::department::DepartmentId::new(billing),
    )
    .await
    .unwrap();
    let refused = setup.fixture.try_publish_agent(setup.agent).await;
    assert!(
        refused.as_ref().err().is_some_and(|error| outside(
            error,
            "publication.resource_outside_unit",
            setup.connection.id
        )),
        "{:?}",
        refused.err()
    );
    place_agent(&setup, billing).await;
    setup.fixture.try_publish_agent(setup.agent).await.unwrap();
}

/// Plan07 D10: a move racing an agent publication either precedes it (the publication sees the
/// new unit and is refused) or follows it (the revision keeps the old unit and admission refuses).
/// Both orders use the production move and publication; the skill row lock orders them.
#[tokio::test]
async fn unit_resource_competing_move_and_agent_publication_in_both_orders() {
    use crate::adapters::persistence::test_support::wait_until_backends_are_blocked;
    use crate::use_cases::skill::SkillManagementPersistence;
    for move_first in [true, false] {
        let (origin, _server) = server(HTTP_OK).await;
        let setup = Box::pin(agent_setup_with(
            &origin,
            Some("Follow the support policy."),
        ))
        .await;
        let p = setup.fixture.persistence.clone();
        let company = setup.fixture.target.company;
        let owner = setup.fixture.target.actor.user_id();
        let skill = agent_skill(&setup).await;
        let billing = p.test_unit(company.as_uuid(), "billing").await;
        let support = p.test_unit(company.as_uuid(), "support").await;
        place_agent(&setup, billing).await;
        SkillManagementPersistence::move_company(&p, owner, company.as_uuid(), skill, billing)
            .await
            .unwrap();
        let published = if move_first {
            // The move holds the skill row; publication's capture waits for it, then sees Support.
            let mut moving = p.pool().begin().await.unwrap();
            crate::adapters::persistence::unit_resources::move_resource_on(
                &mut moving,
                WorkflowActor::authenticated(owner).unwrap(),
                company,
                crate::application::ports::unit_resources::UnitResourceKind::Skill,
                skill,
                crate::entities::department::DepartmentId::new(support),
            )
            .await
            .unwrap();
            let (published, ()) =
                tokio::join!(setup.fixture.try_publish_agent(setup.agent), async {
                    wait_until_backends_are_blocked(p.pool(), 1).await;
                    moving.commit().await.unwrap();
                });
            published
        } else {
            // Publication captures the skill (FOR SHARE) and pauses at activation; the move waits.
            let mut blocker = p.pool().begin().await.unwrap();
            sqlx::query(
                "SELECT 1 FROM agent_active_revisions WHERE company_id=$1 AND agent_id=$2 FOR SHARE",
            )
            .bind(company.as_uuid())
            .bind(setup.agent)
            .execute(&mut *blocker)
            .await
            .unwrap();
            let moving = {
                let p = p.clone();
                async move {
                    SkillManagementPersistence::move_company(
                        &p,
                        owner,
                        company.as_uuid(),
                        skill,
                        support,
                    )
                    .await
                }
            };
            let (published, moved, ()) = tokio::join!(
                setup.fixture.try_publish_agent(setup.agent),
                async {
                    // Start the move only once publication is waiting at activation.
                    wait_until_backends_are_blocked(p.pool(), 1).await;
                    moving.await
                },
                async {
                    wait_until_backends_are_blocked(p.pool(), 2).await;
                    blocker.commit().await.unwrap();
                }
            );
            moved.unwrap();
            published
        };
        if move_first {
            assert!(
                published.as_ref().err().is_some_and(|error| outside(
                    error,
                    "publication.resource_outside_unit",
                    skill
                )),
                "move first: {:?}",
                published.err()
            );
        } else {
            let revision = published.expect("publication first captures the old unit");
            assert_eq!(revision.definition.skills[0].skill.unit_id, billing);
        }
        // Either way the skill is now out of the agent's reach, so admission refuses it.
        let service = WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight);
        let refused = service
            .admit(request(&setup, "unit-competing", Uuid::new_v4()).await)
            .await;
        assert!(
            refused.as_ref().err().is_some_and(|error| outside(
                error,
                "admission.resource_outside_unit",
                skill
            )),
            "{:?}",
            refused.err()
        );
    }
}

/// Plan07 D9b: a model-only use exposes neither tools nor skills, so a skill moved out of the
/// agent's reach does not stop it.
#[tokio::test]
async fn unit_resource_selection_admission_agent_model_only_use_is_unaffected() {
    let (origin, _server) = server(HTTP_OK).await;
    let setup = Box::pin(agent_setup_source(
        &origin,
        Some("Follow the support policy."),
        false,
        |source, _| {
            source["steps"]["start"] = json!({"type":"ai.classify","with":{
                "agent":{"literal":"assistant"},"context":{"literal":null},
                "output_schema":{"literal":{"enum":["yes","no"]}}},"routes":{"success":"$end"}});
        },
    ))
    .await;
    let p = &setup.fixture.persistence;
    let company = setup.fixture.target.company;
    let skill = agent_skill(&setup).await;
    let billing = p.test_unit(company.as_uuid(), "billing").await;
    crate::use_cases::skill::SkillManagementPersistence::move_company(
        p,
        setup.fixture.target.actor.user_id(),
        company.as_uuid(),
        skill,
        billing,
    )
    .await
    .unwrap();
    let service = WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight);
    created(
        service
            .admit(request(&setup, "unit-model-only", Uuid::new_v4()).await)
            .await
            .unwrap(),
    );
}

/// Plan07 D9b with decision d: an agent placed in two units reaches the resources of both,
/// whichever desk the run is on and without one; losing a placement loses its unit's resources.
#[tokio::test]
async fn unit_resource_selection_admission_agent_two_placements_reach_both_on_any_desk() {
    use crate::application::ports::channel::ChannelWrite;
    let (origin, _server) = server(HTTP_OK).await;
    let setup = Box::pin(agent_setup_with(
        &origin,
        Some("Follow the support policy."),
    ))
    .await;
    let p = &setup.fixture.persistence;
    let company = setup.fixture.target.company;
    let billing = p.test_unit(company.as_uuid(), "billing").await;
    let support = p.test_unit(company.as_uuid(), "support").await;
    let billing_skill = agent_skill(&setup).await;
    let support_skill = Uuid::new_v4();
    sqlx::query("INSERT INTO skills(id,company_id,slug,name,description,instructions,created_by,required_tools,unit_id) VALUES($1,$2,'escalation','Escalation','Escalation policy','Escalate politely.','{\"actor_type\":\"system\",\"actor_id\":null,\"actor_name\":\"Fixture\"}','{}',$3)")
        .bind(support_skill).bind(company.as_uuid()).bind(support).execute(p.pool()).await.unwrap();
    sqlx::query(
        "INSERT INTO agent_skills(company_id,agent_id,skill_id,position) VALUES($1,$2,$3,1)",
    )
    .bind(company.as_uuid())
    .bind(setup.agent)
    .bind(support_skill)
    .execute(p.pool())
    .await
    .unwrap();
    sqlx::query("UPDATE skills SET unit_id=$3 WHERE company_id=$1 AND id=$2")
        .bind(company.as_uuid())
        .bind(billing_skill)
        .bind(billing)
        .execute(p.pool())
        .await
        .unwrap();
    place_agent(&setup, billing).await;
    place_agent(&setup, support).await;
    setup.fixture.publish_agent(setup.agent).await;
    let service = WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight);
    let desk = |unit: Uuid| async move {
        crate::adapters::persistence::test_support::create_channel(
            p,
            company.as_uuid(),
            ChannelWrite {
                name: "Desk".into(),
                slug: format!("desk-{}", Uuid::new_v4().simple()),
                department_id: Some(crate::entities::department::DepartmentId::new(unit)),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id
    };
    let desks = [
        RelatedAssociation::Company,
        RelatedAssociation::Channel(crate::application::workflow::RelatedChannelId::new(
            desk(billing).await,
        )),
        RelatedAssociation::Channel(crate::application::workflow::RelatedChannelId::new(
            desk(support).await,
        )),
    ];
    for (index, association) in desks.into_iter().enumerate() {
        let mut admit = request(
            &setup,
            &format!("unit-two-placements-{index}"),
            Uuid::new_v4(),
        )
        .await;
        admit.association = association;
        created(service.admit(admit).await.unwrap());
    }
    sqlx::query("DELETE FROM department_roles WHERE company_id=$1 AND department_id=$2")
        .bind(company.as_uuid())
        .bind(support)
        .execute(p.pool())
        .await
        .unwrap();
    let refused = service
        .admit(request(&setup, "unit-placement-lost", Uuid::new_v4()).await)
        .await;
    assert!(
        refused.as_ref().err().is_some_and(|error| outside(
            error,
            "admission.resource_outside_unit",
            support_skill
        )),
        "{:?}",
        refused.err()
    );
}

/// Plan07 02A with decision d (B1-F1): admission checks the agent's model connection against the
/// agent's reach for every use: moved into Billing, the unplaced agent's run is refused naming
/// it; placed in Billing, or with the connection moved back up, it is admitted.
#[tokio::test]
async fn unit_model_default_admission_refuses_a_model_moved_out_of_reach() {
    let (origin, _server) = server(HTTP_OK).await;
    let setup = Box::pin(agent_setup(&origin)).await;
    let p = &setup.fixture.persistence;
    let company = setup.fixture.target.company.as_uuid();
    let owner = setup.fixture.target.actor.user_id();
    let service = WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight);
    let connection: Uuid = sqlx::query_scalar(
        "SELECT id FROM company_model_connections WHERE company_id=$1 AND provider=$2",
    )
    .bind(company)
    .bind(SCRIPTED_PROVIDER)
    .fetch_one(p.pool())
    .await
    .unwrap();
    let billing = p.test_unit(company, "billing").await;
    let root = p.test_root_unit(company).await;
    p.move_model_connection(owner, company, connection, billing)
        .await
        .unwrap();
    let refused = service
        .admit(request(&setup, "model-moved", Uuid::new_v4()).await)
        .await;
    assert!(
        refused.as_ref().err().is_some_and(|error| outside(
            error,
            "admission.resource_outside_unit",
            connection
        )),
        "{:?}",
        refused.err()
    );
    place_agent(&setup, billing).await;
    created(
        service
            .admit(request(&setup, "model-placed", Uuid::new_v4()).await)
            .await
            .unwrap(),
    );
    p.move_model_connection(owner, company, connection, root)
        .await
        .unwrap();
    created(
        service
            .admit(request(&setup, "model-up", Uuid::new_v4()).await)
            .await
            .unwrap(),
    );
}
