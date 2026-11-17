use super::*;
use crate::application::ports::department::DepartmentRoleAssignment;
use crate::application::workflow::decisions::{
    DecisionCommandKey, DecisionId, SubmitDecision, WorkflowDecisions,
};
use crate::entities::department::RoleCapacity;

struct SponsorPerson {
    user: Uuid,
    principal: PrincipalId,
}
struct SponsorDeployment {
    d: Deployment,
    desks: [RoleDesk; 2],
    bindings: [BindingTarget; 2],
    people: Vec<SponsorPerson>,
}

fn approval_source(follow: bool) -> Value {
    let mut value = source("agent.run");
    let mut review: Value =
        serde_json::from_str(&registry::example("decision.human").unwrap().source).unwrap();
    let mut review = review["steps"]["start"].take();
    review["with"]["proposal"] = json!({"ref":"/steps/start/output"});
    review["with"]["reviewers"] = json!({"literal":["sponsor"]});
    review["with"]["reviewers_follow_role"] = json!({"literal":follow});
    review["with"]["deadline"] = json!({"ref":"/run/deadline"});
    review["with"]["data_schema"] = json!({"literal":true});
    review["with"]["feedback_required"] = json!({"literal":[]});
    review["routes"] = json!({"choices":{"approve":"reply","reject":"reply"}});
    value["steps"]["start"]["routes"] = json!({"success":"review"});
    value["steps"]["review"] = review;
    value["steps"]["reply"] = json!({"type":"data.map","with":{
        "value":{"ref":"/steps/start/output"},"output_schema":{"literal":value["output_schema"].clone()}},
        "routes":{"success":"$end"}});
    value["limits"]["max_steps"] = json!(8);
    value
}

impl SponsorDeployment {
    async fn new(follow: bool) -> Self {
        let mut d = Box::pin(Deployment::new(vec![
            LlmTurn::text(r#""first draft""#),
            LlmTurn::text(r#""second draft""#),
        ]))
        .await;
        let lead = agent(&d, "sponsor-lead").await;
        let desks = [
            desk(&d, "sponsor-first", &lead).await,
            desk(&d, "sponsor-second", &lead).await,
        ];
        let mut people = Vec::new();
        for _ in 0..3 {
            let (user, _) = verified_member(&d.fixture).await;
            sqlx::query("UPDATE users SET email_verified_at=clock_timestamp() WHERE id=$1")
                .bind(user)
                .execute(d.p().pool())
                .await
                .unwrap();
            people.push(SponsorPerson {
                user,
                principal: PrincipalId::new(d.fixture.principal(user).await),
            });
        }
        let departments = crate::adapters::persistence::test_support::department_use_cases(d.p());
        for (index, unit) in desks.iter().enumerate() {
            // Sponsors still need ordinary desk visibility; holding the role grants no access.
            sqlx::query("UPDATE channels SET access_mode='team' WHERE company_id=$1 AND id=$2")
                .bind(d.fixture.target.company.as_uuid())
                .bind(unit.id)
                .execute(d.p().pool())
                .await
                .unwrap();
            for principal in [people[index].principal, people[2].principal, lead.principal] {
                departments
                    .assign_role(
                        d.fixture.target.actor.user_id(),
                        d.fixture.target.company.as_uuid(),
                        unit.department,
                        DepartmentRoleAssignment {
                            role: DepartmentRole::Sponsor,
                            principal,
                            decider: false,
                        },
                    )
                    .await
                    .unwrap();
            }
        }
        d.channel = desks[0].id;
        let first = d
            .bind(approval_source(follow), Some(MessageTrigger::Any))
            .await;
        let second = bind_same_version(&d, first, &desks[1]).await;
        Self {
            d,
            desks,
            bindings: [first, second],
            people,
        }
    }
    async fn parked(&self) -> [RunId; 2] {
        let first = admit(
            &self.d,
            self.bindings[0],
            &self.desks[0],
            "sponsor-first",
            Uuid::new_v4(),
        )
        .await;
        let second = admit(
            &self.d,
            self.bindings[1],
            &self.desks[1],
            "sponsor-second",
            Uuid::new_v4(),
        )
        .await;
        Box::pin(self.d.run_until("both sponsor decisions parked", async || {
            self.d
                .count("SELECT count(DISTINCT decision.id) FROM workflow_decisions AS decision \
                    JOIN workflow_decision_reviewers AS reviewer ON reviewer.company_id=decision.company_id AND reviewer.decision_id=decision.id \
                    JOIN workflow_waits AS wait ON wait.company_id=decision.company_id AND wait.id=decision.wait_id \
                    WHERE decision.company_id=$1 AND decision.subject_kind='business' AND wait.state='waiting'")
                .await
                == 2
        }))
        .await;
        [first, second]
    }
    async fn decision(&self, run: RunId) -> DecisionId {
        DecisionId(
            sqlx::query_scalar(
                "SELECT id FROM workflow_decisions WHERE company_id=$1 AND run_id=$2",
            )
            .bind(self.bindings[0].company.as_uuid())
            .bind(run.as_uuid())
            .fetch_one(self.d.p().pool())
            .await
            .unwrap(),
        )
    }
    async fn deciders(&self, decision: DecisionId) -> Vec<Uuid> {
        sqlx::query_scalar("SELECT principal_id FROM workflow_decision_reviewers WHERE company_id=$1 AND decision_id=$2 ORDER BY principal_id")
            .bind(self.bindings[0].company.as_uuid()).bind(decision.0).fetch_all(self.d.p().pool()).await.unwrap()
    }
    async fn move_decider(&self) {
        crate::adapters::persistence::test_support::department_use_cases(self.d.p())
            .assign_role(
                self.d.fixture.target.actor.user_id(),
                self.bindings[0].company.as_uuid(),
                self.desks[0].department,
                DepartmentRoleAssignment {
                    role: DepartmentRole::Sponsor,
                    principal: self.people[2].principal,
                    decider: true,
                },
            )
            .await
            .unwrap();
    }
    fn worker(&self) -> crate::application::services::notification_worker::NotificationWorker {
        crate::adapters::persistence::test_support::notification_worker(
            self.d.p(),
            Default::default(),
        )
    }
}

async fn assert_information_notices(f: &SponsorDeployment) {
    assert_eq!(f.worker().drain_once().await.unwrap(), 2);
    let informed:Vec<Uuid>=sqlx::query_scalar("SELECT recipient_principal_id FROM notifications WHERE company_id=$1 AND action_kind='decision_information' ORDER BY recipient_principal_id")
        .bind(f.bindings[0].company.as_uuid()).fetch_all(f.d.p().pool()).await.unwrap();
    assert_eq!(
        informed,
        vec![f.people[2].principal.as_uuid(); 2],
        "informed agent never gets a notice"
    );
    let parts:String=sqlx::query_scalar("SELECT jsonb_agg(to_jsonb(part))::text FROM notifications AS notification JOIN message_delivery_parts AS part ON part.company_id=notification.company_id AND part.delivery_id=notification.email_delivery_id WHERE notification.company_id=$1 AND notification.action_kind='decision_information'")
        .bind(f.bindings[0].company.as_uuid()).fetch_one(f.d.p().pool()).await.unwrap();
    assert!(parts.to_lowercase().contains("for information only"));
    let subjects: Vec<String> = sqlx::query_scalar(
        "SELECT part.payload->'body'->>'subject' FROM notifications AS notification \
         JOIN message_delivery_parts AS part ON part.company_id=notification.company_id \
           AND part.delivery_id=notification.email_delivery_id \
         WHERE notification.company_id=$1 AND notification.action_kind='decision_information'",
    )
    .bind(f.bindings[0].company.as_uuid())
    .fetch_all(f.d.p().pool())
    .await
    .unwrap();
    assert!(!subjects.is_empty());
    for subject in subjects {
        assert_eq!(subject, "Workflow decision — for information only");
    }
    assert!(
        !parts.contains("first draft"),
        "information notice never quotes the proposal"
    );
}

async fn assert_information_cannot_settle(f: &SponsorDeployment, first: DecisionId) {
    let actor = WorkflowActor::authenticated(f.people[2].user).unwrap();
    let view =
        f.d.p()
            .read_decision(f.bindings[0].company, actor, first)
            .await
            .unwrap();
    assert!(!view.can_submit);
    let result =
        f.d.p()
            .submit_decision(SubmitDecision {
                company: f.bindings[0].company,
                actor,
                decision: first,
                command_key: DecisionCommandKey::parse("informed-cannot-settle").unwrap(),
                expected_revision: view.revision,
                expected_artifact: view.artifact_digest,
                choice: view.choices[0].clone(),
                feedback: String::new(),
                data: json!(null),
            })
            .await;
    assert!(
        matches!(result, Err(AppError::NotFound(ref resource)) if resource == "Workflow resource"),
        "{result:?}"
    );
    let unsettled: bool = sqlx::query_scalar(
        "SELECT submitted_at IS NULL FROM workflow_decisions WHERE company_id=$1 AND id=$2",
    )
    .bind(f.bindings[0].company.as_uuid())
    .bind(first.0)
    .fetch_one(f.d.p().pool())
    .await
    .unwrap();
    assert!(unsettled, "an informed holder cannot settle the decision");
}

#[tokio::test]
async fn workflow_role_sponsor_admission_two_units_freeze_deciders_information_and_settlement_authority()
 {
    let f = Box::pin(SponsorDeployment::new(false)).await;
    let runs = f.parked().await;
    let first = f.decision(runs[0]).await;
    let second = f.decision(runs[1]).await;
    assert_eq!(
        f.deciders(first).await,
        vec![f.people[0].principal.as_uuid()]
    );
    assert_eq!(
        f.deciders(second).await,
        vec![f.people[1].principal.as_uuid()]
    );
    let bindings:Vec<(String,Option<Uuid>)>=sqlx::query_as("SELECT capacity,agent_id FROM workflow_run_role_bindings WHERE company_id=$1 AND run_id=$2 AND role='sponsor' ORDER BY capacity,principal_id")
        .bind(f.bindings[0].company.as_uuid()).bind(runs[0].as_uuid()).fetch_all(f.d.p().pool()).await.unwrap();
    assert_eq!(bindings.len(), 3);
    assert_eq!(
        bindings
            .iter()
            .filter(|row| row.0 == RoleCapacity::Decides.as_str())
            .count(),
        1
    );
    assert_eq!(
        bindings
            .iter()
            .filter(|row| row.0 == RoleCapacity::Informed.as_str())
            .count(),
        2
    );
    assert!(bindings.iter().all(|row| row.1.is_none()));
    // Box composed assertion futures so the integration test retains its stock stack budget.
    Box::pin(assert_information_notices(&f)).await;
    Box::pin(assert_information_cannot_settle(&f, first)).await;
    f.move_decider().await;
    assert_eq!(
        f.deciders(first).await,
        vec![f.people[0].principal.as_uuid()],
        "without follow-role frozen decider stays"
    );
    assert_eq!(f.d.llm.finish().await.unwrap(), 2);
}

#[tokio::test]
async fn workflow_role_sponsor_admission_follow_replaces_decider_and_information_in_one_commit() {
    let f = Box::pin(SponsorDeployment::new(true)).await;
    let runs = f.parked().await;
    let first = f.decision(runs[0]).await;
    f.worker().drain_once().await.unwrap();
    let before:Value=sqlx::query_scalar("SELECT jsonb_build_object('proposal',proposal_digest,'revision',revision) FROM workflow_decisions WHERE company_id=$1 AND id=$2")
        .bind(f.bindings[0].company.as_uuid()).bind(first.0).fetch_one(f.d.p().pool()).await.unwrap();
    f.move_decider().await;
    assert_eq!(
        f.deciders(first).await,
        vec![f.people[2].principal.as_uuid()]
    );
    let current:Vec<Uuid>=sqlx::query_scalar("SELECT principal_id FROM workflow_decision_information_notices WHERE company_id=$1 AND decision_id=$2 AND active")
        .bind(f.bindings[0].company.as_uuid()).bind(first.0).fetch_all(f.d.p().pool()).await.unwrap();
    assert_eq!(current, vec![f.people[0].principal.as_uuid()]);
    let after:Value=sqlx::query_scalar("SELECT jsonb_build_object('proposal',proposal_digest,'revision',revision) FROM workflow_decisions WHERE company_id=$1 AND id=$2")
        .bind(f.bindings[0].company.as_uuid()).bind(first.0).fetch_one(f.d.p().pool()).await.unwrap();
    assert_eq!(after, before);
    let inactive:bool=sqlx::query_scalar("SELECT count(*)=1 AND bool_and(NOT source.active AND notification.state='withdrawn' AND delivery.cancellation_reason='recipient_revoked') FROM workflow_decision_information_notices AS source JOIN notifications AS notification ON notification.company_id=source.company_id AND notification.event_id=source.event_id JOIN message_deliveries AS delivery ON delivery.id=notification.email_delivery_id WHERE source.company_id=$1 AND source.decision_id=$2 AND source.principal_id=$3")
        .bind(f.bindings[0].company.as_uuid()).bind(first.0).bind(f.people[2].principal.as_uuid()).fetch_one(f.d.p().pool()).await.unwrap();
    assert!(inactive);
    assert_eq!(f.d.llm.finish().await.unwrap(), 2);
}
