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
    sqlx::query("UPDATE agents SET slug='renamed-assistant' WHERE company_id=$1 AND id=$2")
        .bind(scope(&setup).company.as_uuid())
        .bind(setup.agent)
        .execute(p.pool())
        .await
        .unwrap();
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
