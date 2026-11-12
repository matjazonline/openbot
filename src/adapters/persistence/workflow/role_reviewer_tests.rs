use super::*;
#[path = "escalation_recipient_storage_tests.rs"]
mod escalation_recipients;
use crate::adapters::persistence::workflow::tests::approval_test_support::notices;
use crate::application::workflow::activation::{ActivationRequest, WorkflowJobId};
use crate::application::workflow::decisions::{
    DecisionCommandKey, DecisionId, SubmitDecision, WorkflowDecisions,
};

#[derive(sqlx::FromRow)]
struct ScopeIds {
    execution: Uuid,
    job: Uuid,
}

struct PeopleFixture {
    role: RoleFixture,
    users: Vec<Uuid>,
    principals: Vec<PrincipalId>,
}
enum ReviewerTracking {
    Frozen,
    FollowRole,
}

impl PeopleFixture {
    fn new(count: usize) -> impl std::future::Future<Output = Self> {
        Self::with_reviewers(count, json!(["reviewer"]))
    }
    fn with_reviewers(count: usize, reviewers: Value) -> impl std::future::Future<Output = Self> {
        Self::with_tracking(count, reviewers, ReviewerTracking::Frozen)
    }
    async fn with_tracking(count: usize, reviewers: Value, tracking: ReviewerTracking) -> Self {
        let fixture = Fixture::new().await;
        let mut users = Vec::new();
        let mut principals = Vec::new();
        for _ in 0..count {
            let user =
                crate::adapters::persistence::workflow::tests::message_tests::support::member(
                    &fixture, "admin",
                )
                .await;
            sqlx::query("UPDATE users SET email_verified_at=clock_timestamp() WHERE id=$1")
                .bind(user)
                .execute(fixture.persistence.pool())
                .await
                .unwrap();
            users.push(user);
            principals.push(PrincipalId::new(fixture.principal(user).await));
        }
        let mut source: Value =
            serde_json::from_str(&registry::example("decision.human").unwrap().source).unwrap();
        source["input_schema"] = json!({"type":"object","properties":{"value":{"type":"integer"}},"required":["value"],"additionalProperties":false});
        source["steps"]["start"]["with"]["data_schema"] = json!({"literal":true});
        source["steps"]["start"]["with"]["feedback_required"] = json!({"literal":[]});
        source["steps"]["start"]["with"]["reviewers"] = json!({"literal":reviewers});
        if matches!(tracking, ReviewerTracking::FollowRole) {
            source["steps"]["start"]["with"]["reviewers_follow_role"] = json!({"literal":true});
        }
        source["steps"]["start"]["with"]["deadline"] =
            json!({"literal":(chrono::Utc::now()+chrono::Duration::minutes(20)).to_rfc3339()});
        let binding = BindingFixture::from_fixture(fixture, source).await;
        let role = RoleFixture::from_binding(binding).await;
        let departments =
            crate::adapters::persistence::test_support::department_use_cases(role.p());
        for principal in &principals {
            departments
                .add_member(
                    role.admission.binding.target.actor.user_id(),
                    role.company().as_uuid(),
                    role.department,
                    *principal,
                )
                .await
                .unwrap();
            departments
                .assign_role(
                    role.admission.binding.target.actor.user_id(),
                    role.company().as_uuid(),
                    role.department,
                    DepartmentRole::Reviewer,
                    *principal,
                )
                .await
                .unwrap();
        }
        Self {
            role,
            users,
            principals,
        }
    }
    async fn scope(&self, run: RunId) -> ActivationRequest {
        let row: ScopeIds = sqlx::query_as("SELECT execution.id AS execution,job.id AS job FROM workflow_executions AS execution JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id WHERE execution.company_id=$1 AND execution.run_id=$2")
            .bind(self.role.company().as_uuid()).bind(run.as_uuid()).fetch_one(self.role.p().pool()).await.unwrap();
        ActivationRequest {
            company: self.role.company(),
            run,
            execution: ExecutionId::new(row.execution),
            job: WorkflowJobId(row.job),
        }
    }
}

async fn republish(f: &PeopleFixture, mut source: Value) -> VersionId {
    let fixture = &f.role.admission.binding.fixture;
    let detail = fixture
        .persistence
        .definition(fixture.target.company, fixture.target.workflow)
        .await
        .unwrap()
        .unwrap();
    source["workflow_id"] = json!(fixture.target.workflow);
    let service = crate::application::workflow::lifecycle::DefinitionService::new(
        Preflight,
        fixture.persistence.clone(),
        crate::adapters::workflow_source::WorkflowSourceDecoder,
        crate::adapters::persistence::workflow::agent_publication::AgentPublicationDirectory {
            persistence: fixture.persistence.clone(),
            inner: crate::adapters::persistence::workflow::child_publication::ChildPublicationDirectory {
                persistence: fixture.persistence.clone(),
                inner: Preflight,
            },
        },
    );
    let mut draft = fixture.save(Some(
        crate::domain::workflow::DraftRevision::new(detail.summary.revision).unwrap(),
    ));
    draft.content.source = source.to_string();
    service.save(draft).await.unwrap();
    let mut publication = fixture.publish();
    publication.expected =
        crate::domain::workflow::DraftRevision::new(detail.summary.revision + 1).unwrap();
    publication.key = crate::application::workflow::IdempotencyKey::parse(format!(
        "p03-republish-{}",
        publication.version.as_uuid()
    ))
    .unwrap();
    let version = publication.version;
    service.publish(publication).await.unwrap();
    version
}

#[tokio::test]
async fn workflow_role_reviewer_captures_all_people_parks_notices_labels_and_submits() {
    let f = Box::pin(PeopleFixture::new(2)).await;
    let run = f.role.admitted("people").await;
    let manifest = f.role.manifest(run).await;
    assert_eq!(manifest.roles.len(), 1);
    assert_eq!(manifest.roles[0].principals.len(), 2);
    assert!(
        manifest.roles[0]
            .principals
            .iter()
            .all(|holder| holder.agent_id.is_none() && holder.revision_id.is_none())
    );
    let scope = f.scope(run).await;
    let parked = f
        .role
        .p()
        .park_decision(scope, &notices(f.role.p()))
        .await
        .unwrap();
    let state: Value = sqlx::query_scalar("SELECT to_jsonb(execution) FROM workflow_executions AS execution WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.execution.as_uuid()).fetch_one(f.role.p().pool()).await.unwrap();
    assert!(parked.is_some(), "decision did not park: {state}");
    let decision: Uuid =
        sqlx::query_scalar("SELECT id FROM workflow_decisions WHERE company_id=$1 AND run_id=$2")
            .bind(scope.company.as_uuid())
            .bind(run.as_uuid())
            .fetch_one(f.role.p().pool())
            .await
            .unwrap();
    let actor = WorkflowActor::authenticated(f.users[0]).unwrap();
    let view = f
        .role
        .p()
        .read_decision(scope.company, actor, DecisionId(decision))
        .await
        .unwrap();
    assert_eq!(view.reviewers.len(), 2);
    assert_eq!(view.reviewer_labels.len(), 2);
    assert!(view.can_submit);
    assert!(
        view.reviewers
            .iter()
            .all(|id| f.principals.contains(&PrincipalId::new(*id)))
    );
    let deliveries: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_decision_reviewers WHERE company_id=$1 AND decision_id=$2 AND notice_delivery_id IS NOT NULL")
        .bind(scope.company.as_uuid()).bind(decision).fetch_one(f.role.p().pool()).await.unwrap();
    assert_eq!(deliveries, 2);
    f.role
        .p()
        .submit_decision(SubmitDecision {
            company: scope.company,
            actor,
            decision: DecisionId(decision),
            command_key: DecisionCommandKey::parse("role-vote").unwrap(),
            expected_revision: view.revision,
            expected_artifact: view.artifact_digest,
            choice: view.choices[0].clone(),
            data: json!(null),
            feedback: String::new(),
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn workflow_role_reviewer_missing_holder_refuses_before_run() {
    let f = Box::pin(PeopleFixture::new(0)).await;
    let command = f.role.prepared("missing-people").await;
    assert!(
        matches!(f.role.p().admit(&command).await,Err(AppError::WorkflowRole(refusal)) if refusal.kind==RoleRefusalKind::NoHolder && refusal.role==DepartmentRole::Reviewer)
    );
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workflow_runs WHERE company_id=$1 AND id=$2)",
    )
    .bind(f.role.company().as_uuid())
    .bind(command.proposed_run_id().as_uuid())
    .fetch_one(f.role.p().pool())
    .await
    .unwrap();
    assert!(!exists);
}

#[tokio::test]
async fn workflow_role_reviewer_frozen_members_survive_role_reassignment_before_park() {
    let f = Box::pin(PeopleFixture::new(2)).await;
    let run = f.role.admitted("frozen-people").await;
    let departments = crate::adapters::persistence::test_support::department_use_cases(f.role.p());
    for principal in &f.principals {
        departments
            .clear_role(
                f.role.admission.binding.target.actor.user_id(),
                f.role.company().as_uuid(),
                f.role.department,
                DepartmentRole::Reviewer,
                *principal,
            )
            .await
            .unwrap();
    }
    let newcomer = crate::adapters::persistence::workflow::tests::message_tests::support::member(
        &f.role.admission.binding.fixture,
        "admin",
    )
    .await;
    let principal = PrincipalId::new(f.role.admission.binding.fixture.principal(newcomer).await);
    departments
        .assign_role(
            f.role.admission.binding.target.actor.user_id(),
            f.role.company().as_uuid(),
            f.role.department,
            DepartmentRole::Reviewer,
            principal,
        )
        .await
        .unwrap();
    let scope = f.scope(run).await;
    assert!(
        f.role
            .p()
            .park_decision(scope, &notices(f.role.p()))
            .await
            .unwrap()
            .is_some()
    );
    let saved:Vec<Uuid>=sqlx::query_scalar("SELECT reviewer.principal_id FROM workflow_decision_reviewers AS reviewer JOIN workflow_decisions AS decision ON decision.company_id=reviewer.company_id AND decision.id=reviewer.decision_id WHERE decision.company_id=$1 AND decision.run_id=$2 ORDER BY reviewer.principal_id")
        .bind(scope.company.as_uuid()).bind(run.as_uuid()).fetch_all(f.role.p().pool()).await.unwrap();
    assert_eq!(saved.len(), 2);
    assert!(
        saved
            .iter()
            .all(|id| f.principals.contains(&PrincipalId::new(*id)))
    );
    assert!(!saved.contains(&principal.as_uuid()));
}

#[tokio::test]
async fn workflow_role_reviewer_effective_union_deduplicates_sixteen_and_refuses_seventeen() {
    for extra in [false, true] {
        let f = Box::pin(PeopleFixture::with_reviewers(
            16,
            json!(["lead", "reviewer"]),
        ))
        .await;
        let departments =
            crate::adapters::persistence::test_support::department_use_cases(f.role.p());
        let lead = if extra {
            let user =
                crate::adapters::persistence::workflow::tests::message_tests::support::member(
                    &f.role.admission.binding.fixture,
                    "admin",
                )
                .await;
            PrincipalId::new(f.role.admission.binding.fixture.principal(user).await)
        } else {
            f.principals[0]
        };
        departments
            .assign_role(
                f.role.admission.binding.target.actor.user_id(),
                f.role.company().as_uuid(),
                f.role.department,
                DepartmentRole::Lead,
                lead,
            )
            .await
            .unwrap();
        let command = f.role.prepared("union-bound").await;
        let admitted = f.role.p().admit(&command).await;
        if extra {
            assert!(
                matches!(admitted,Err(AppError::WorkflowRole(refusal)) if refusal.kind==RoleRefusalKind::TooMany)
            );
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM workflow_runs WHERE company_id=$1 AND id=$2)",
            )
            .bind(f.role.company().as_uuid())
            .bind(command.proposed_run_id().as_uuid())
            .fetch_one(f.role.p().pool())
            .await
            .unwrap();
            assert!(!exists);
        } else {
            assert!(matches!(admitted, Ok(AdmissionResult::Created(_))));
            let scope = f.scope(command.proposed_run_id()).await;
            assert!(
                f.role
                    .p()
                    .park_decision(scope, &notices(f.role.p()))
                    .await
                    .unwrap()
                    .is_some()
            );
            let count:i64=sqlx::query_scalar("SELECT count(*) FROM workflow_decision_reviewers AS reviewer JOIN workflow_decisions AS decision ON decision.company_id=reviewer.company_id AND decision.id=reviewer.decision_id WHERE decision.company_id=$1 AND decision.run_id=$2")
                .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_one(f.role.p().pool()).await.unwrap();
            assert_eq!(
                count, 16,
                "overlapping roles yield one row and notice per effective person"
            );
        }
    }
}

#[tokio::test]
async fn workflow_role_reviewer_sql_missing_one_captured_person_rolls_back_admission() {
    let f = Box::pin(PeopleFixture::new(2)).await;
    let omitted = f.principals[0].as_uuid();
    let hook = format!(
        "CREATE FUNCTION omit_one_person() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.principal_id='{omitted}'::uuid THEN RETURN NULL; END IF; RETURN NEW; END $$; CREATE TRIGGER omit_one_person BEFORE INSERT ON workflow_run_role_bindings FOR EACH ROW EXECUTE FUNCTION omit_one_person()"
    );
    sqlx::raw_sql(&hook)
        .execute(f.role.p().pool())
        .await
        .unwrap();
    let command = f.role.prepared("incomplete-people").await;
    let error = f.role.p().admit(&command).await.unwrap_err();
    assert!(
        matches!(error,AppError::WorkflowGuard(ref violation) if violation.guard()=="check_workflow_role_manifest"&&violation.message().contains("role members must exactly match")),
        "{error:?}"
    );
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workflow_runs WHERE company_id=$1 AND id=$2)",
    )
    .bind(f.role.company().as_uuid())
    .bind(command.proposed_run_id().as_uuid())
    .fetch_one(f.role.p().pool())
    .await
    .unwrap();
    assert!(!exists);
}

#[tokio::test]
async fn workflow_role_reviewer_sql_late_person_cannot_extend_the_sealed_member_set() {
    let f = Box::pin(PeopleFixture::new(2)).await;
    let run = f.role.admitted("sealed-people").await;
    let user = crate::adapters::persistence::workflow::tests::message_tests::support::member(
        &f.role.admission.binding.fixture,
        "admin",
    )
    .await;
    let extra = f.role.admission.binding.fixture.principal(user).await;
    let mut tx = f.role.p().pool().begin().await.unwrap();
    sqlx::query("INSERT INTO workflow_run_role_bindings(company_id,run_id,manifest_id,version_id,step,role,department_id,principal_id) SELECT company_id,run_id,manifest_id,version_id,step,role,department_id,$3 FROM workflow_run_role_bindings WHERE company_id=$1 AND run_id=$2 AND role='reviewer' LIMIT 1")
        .bind(f.role.company().as_uuid()).bind(run.as_uuid()).bind(extra).execute(&mut *tx).await.unwrap();
    let error = sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(&mut *tx)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("workflow_guard:check_workflow_role_manifest")
    );
    tx.rollback().await.unwrap();
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_run_role_bindings WHERE company_id=$1 AND run_id=$2",
    )
    .bind(f.role.company().as_uuid())
    .bind(run.as_uuid())
    .fetch_one(f.role.p().pool())
    .await
    .unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn workflow_reviewer_principal_owner_and_missing_member_removal_keep_review_notices() {
    use crate::use_cases::company_invite::CompanyInvitePersistence;
    let f = Box::pin(PeopleFixture::new(0)).await;
    let owner = f.role.admission.binding.target.actor.user_id();
    let principal = PrincipalId::new(f.role.admission.binding.fixture.principal(owner).await);
    let departments = crate::adapters::persistence::test_support::department_use_cases(f.role.p());
    departments
        .assign_role(
            owner,
            f.role.company().as_uuid(),
            f.role.department,
            DepartmentRole::Reviewer,
            principal,
        )
        .await
        .unwrap();
    let run = f.role.admitted("owner-reviewer").await;
    let scope = f.scope(run).await;
    assert!(
        f.role
            .p()
            .park_decision(scope, &notices(f.role.p()))
            .await
            .unwrap()
            .is_some()
    );
    let notice:Uuid=sqlx::query_scalar("SELECT reviewer.notice_delivery_id FROM workflow_decision_reviewers AS reviewer JOIN workflow_decisions AS decision ON decision.company_id=reviewer.company_id AND decision.id=reviewer.decision_id WHERE decision.company_id=$1 AND decision.run_id=$2")
        .bind(scope.company.as_uuid()).bind(run.as_uuid()).fetch_one(f.role.p().pool()).await.unwrap();
    for user in [owner, Uuid::new_v4()] {
        f.role
            .p()
            .remove_member(f.role.company().as_uuid(), user)
            .await
            .unwrap();
        let pending:bool=sqlx::query_scalar("SELECT status='pending' AND cancellation_requested_at IS NULL FROM message_deliveries WHERE company_id=$1 AND id=$2")
            .bind(scope.company.as_uuid()).bind(notice).fetch_one(f.role.p().pool()).await.unwrap();
        assert!(pending);
    }
}

#[path = "role_reviewer_child_tests.rs"]
mod child_tests;

#[path = "people_resolution_tests.rs"]
mod resolution_tests;

#[tokio::test]
async fn workflow_role_reviewer_people_approval_does_not_replace_a_pinned_model_agent() {
    use crate::application::workflow::reviewers::ReviewerSelector;
    let f = Box::pin(PeopleFixture::new(2)).await;
    let mut source: Value =
        serde_json::from_str(&registry::example("agent.run").unwrap().source).unwrap();
    source["input_schema"] = json!(true);
    source["steps"]["start"]["with"]["agent"] = json!({"literal":"original"});
    source["steps"]["start"]["with"]["approval"] =
        json!({"literal":{"reviewers":["reviewer"],"deadline":"2099-01-01T00:00:00Z"}});
    let version = republish(&f, source).await;
    let binding = &f.role.admission.binding;
    let mut configuration = binding.request(Some(binding.state().await.revision));
    configuration.version = version;
    configuration.association = RelatedAssociation::Channel(RelatedChannelId::new(f.role.channel));
    binding.service().configure(configuration).await.unwrap();
    let command = f.role.prepared("pinned-with-people").await;
    f.role.p().admit(&command).await.unwrap();
    let run = command.proposed_run_id();
    let scope = f.scope(run).await;
    let bundle = command.binding().bundle();
    assert_eq!(bundle.compiled().role_references().len(), 1);
    assert_eq!(
        bundle.compiled().role_references()[0].mode,
        crate::application::workflow::publication::RoleUseMode::People
    );
    let mut db = f.role.p().pool().acquire().await.unwrap();
    let agent = crate::adapters::persistence::workflow::agent_manifest::agent_for_step_on(
        &mut db,
        f.role.company(),
        run,
        bundle,
        &StepId::parse("start").unwrap(),
        &json!({"agent":"original"}),
    )
    .await
    .unwrap();
    assert_eq!(agent.as_uuid(), f.role.agent);
    let reviewers = crate::adapters::persistence::workflow::reviewer_resolution::resolve_on(
        &mut db,
        scope,
        &[ReviewerSelector::Role(DepartmentRole::Reviewer)],
    )
    .await
    .unwrap();
    assert_eq!(reviewers.len(), 2);
    assert!(
        reviewers
            .iter()
            .all(|id| f.principals.contains(&PrincipalId::new(*id)))
    );
}

#[path = "follow_reviewer_tests.rs"]
mod follow_tests;
