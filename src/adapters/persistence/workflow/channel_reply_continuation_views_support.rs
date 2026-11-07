//! Genuine admissions, question publication/replies and child parking for outcome regressions.
use super::*;

struct Prompt {
    pub(super) scope: ActivationRequest,
    pub(super) message: CanonicalMessageId,
    pub(super) wait: crate::domain::workflow::WaitId,
    pub(super) respondent: Uuid,
    pub(super) delivery: Option<crate::entities::transport::DeliveryId>,
}

#[derive(sqlx::FromRow)]
struct PromptRow {
    pub(super) message_id: Uuid,
    pub(super) wait_id: Uuid,
    pub(super) respondent_id: Uuid,
    pub(super) delivery_id: Option<Uuid>,
}

async fn ask(f: &FinalFixture, scope: ActivationRequest) -> Prompt {
    let claim =
        f.p.claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
            .await
            .unwrap()
            .unwrap();
    let result = runtime(&f.p)
        .execute(&WorkflowStepKind("message.ask".into()), &claim)
        .await;
    let WorkflowHandlerOutcome::Suspended(parked) = result else {
        panic!("{result:?}");
    };
    assert!(
        f.p.confirm_suspension(claim.fence, parked.wait)
            .await
            .unwrap()
    );
    let row: PromptRow = sqlx::query_as("SELECT question.message_id,question.wait_id,question.respondent_id,destination.delivery_id FROM workflow_channel_questions AS question JOIN workflow_message_destinations AS destination ON destination.company_id=question.company_id AND destination.invocation_id=question.invocation_id WHERE question.company_id=$1 AND question.execution_id=$2")
        .bind(scope.company.as_uuid()).bind(scope.execution.as_uuid()).fetch_one(f.p.pool()).await.unwrap();
    Prompt {
        scope,
        message: CanonicalMessageId::new(row.message_id),
        wait: crate::domain::workflow::WaitId::new(row.wait_id),
        respondent: row.respondent_id,
        delivery: row
            .delivery_id
            .map(crate::entities::transport::DeliveryId::new),
    }
}

struct Answer {
    pub(super) message: CanonicalMessageId,
    pub(super) next: ActivationRequest,
}

async fn answer(f: &FinalFixture, prompt: &Prompt) -> Answer {
    let incoming =
        f.p.create_message(&inbound(
            f.s.thread,
            &format!("<outcome-answer-{}@partner.test>", Uuid::new_v4()),
            &[],
            "Human answer",
        ))
        .await
        .unwrap()
        .canonical_id;
    assert_eq!(
        f.p.deliver_reply(IngressReply {
            company: f.scope.company,
            thread: f.s.thread,
            answers: prompt.message,
            responder: Some(prompt.respondent),
            in_app: false,
            event: WorkflowEventId(Uuid::new_v4()),
            payload: MessageTriggerInput::new(
                incoming,
                "Human answer",
                "Question",
                "customer@partner.test"
            )
            .to_value(),
            park_grace_until: Utc::now() + chrono::Duration::minutes(5),
        })
        .await
        .unwrap(),
        ReplyIngress::Recorded(prompt.scope.run)
    );
    let WaitProgress::Completed(done) = f.p.resume_wait(prompt.scope).await.unwrap() else {
        panic!("Actual answer resumes the question")
    };
    Answer {
        message: incoming,
        next: done.successor.unwrap(),
    }
}

fn question_source() -> Value {
    let mut source = final_source("message.ask");
    let inputs = json!({"source_message":{"ref":"/input/source_message"},
        "content":{"literal":{"body":"First question"}},
        "deadline":{"ref":"/run/deadline"},"payload_schema":{"literal":MessageTriggerInput::schema()}});
    source["steps"]["start"]["with"] = inputs.clone();
    source["steps"]["start"]["routes"] = json!({"success":"again"});
    source["steps"]["again"] =
        json!({"type":"message.ask","with":inputs,"routes":{"success":"finish"}});
    source["steps"]["again"]["with"]["source_message"] =
        json!({"ref":"/steps/start/output/source_message"});
    source["steps"]["finish"] = json!({"type":"channel.no_reply","with":{
        "value":{"literal":{"body":"Done"}},"output_schema":{"literal":true}},"routes":{"success":"$end"}});
    source
}

pub(super) async fn questions() -> FinalFixture {
    let source = question_source();
    Box::pin(FinalFixture::from_case(
        Box::pin(classified_intents()).await,
        FinalCase {
            source,
            contract: literal_contract("Final"),
            policy: None,
            run_as: None,
            publication: FinalPublication::Messaging,
            delivery_policy: DeliveryPolicy::Deliver,
        },
    ))
    .await
}

async fn uncertain_prompt(f: &FinalFixture, delivery: crate::entities::transport::DeliveryId) {
    let claim =
        f.p.claim_deliveries(
            crate::transport::WorkerId::random(),
            Duration::from_secs(120),
            64,
        )
        .await
        .unwrap()
        .into_iter()
        .find(|claim| claim.record.id == delivery)
        .unwrap();
    assert!(
        f.p.claim_deliveries(
            crate::transport::WorkerId::random(),
            Duration::from_secs(120),
            64
        )
        .await
        .unwrap()
        .is_empty(),
        "The closed first prompt is no longer deliverable"
    );
    f.p.begin_part(&claim.lease, claim.parts[0].id)
        .await
        .unwrap();
    let uncertain = ProviderSendOutcome::OutcomeUnknown {
        class: FailureClass::Timeout,
        detail: FailureDetail::parse("unconfirmed").unwrap(),
    };
    assert_eq!(
        f.p.complete_part(PartResult {
            fence: &claim.lease,
            part_id: claim.parts[0].id,
            outcome: &uncertain
        })
        .await
        .unwrap(),
        DeliveryOutcome::Applied(DeliveryStatus::OutcomeUnknown)
    );
}

pub(super) async fn two_questions(f: &FinalFixture, scope: ActivationRequest) -> ActivationRequest {
    let first = Box::pin(ask(f, scope)).await;
    let initial = outcome(f).await;
    assert_eq!(initial.prompt.unwrap().message, first.message);
    let answered = Box::pin(answer(f, &first)).await;
    let second = Box::pin(ask(f, answered.next)).await;
    let actual_source: Uuid = sqlx::query_scalar("SELECT source_message_id FROM workflow_channel_questions WHERE company_id=$1 AND wait_id=$2")
        .bind(f.scope.company.as_uuid()).bind(second.wait.as_uuid()).fetch_one(f.p.pool()).await.unwrap();
    assert_eq!(actual_source, answered.message.as_uuid());
    let current = outcome(f).await;
    assert_eq!(current.source, initial.source);
    let prompt = current.prompt.as_ref().unwrap();
    assert_eq!(
        prompt.message, second.message,
        "The active second question must replace the closed first question"
    );
    assert_eq!(prompt.wait, second.wait);
    assert!(prompt.waiting && !prompt.awaiting_viewer);
    let respondent: Uuid =
        sqlx::query_scalar("SELECT user_id FROM principals WHERE company_id=$1 AND id=$2")
            .bind(f.scope.company.as_uuid())
            .bind(second.respondent)
            .fetch_one(f.p.pool())
            .await
            .unwrap();
    let viewer = WorkflowActor::authenticated(respondent).unwrap();
    let own = channel_reply_views::read(&f.p, f.scope.company, viewer, &[f.scope.run.as_uuid()])
        .await
        .unwrap();
    assert!(own[0].prompt.as_ref().unwrap().awaiting_viewer);
    assert_eq!(own[0].label(), "Waiting for your response");
    Box::pin(uncertain_prompt(
        f,
        second.delivery.expect("email prompt has a delivery"),
    ))
    .await;
    let attention = outcome(f).await;
    assert_eq!(
        attention
            .prompt
            .as_ref()
            .unwrap()
            .delivery
            .as_ref()
            .unwrap()
            .state,
        DeliveryStatus::OutcomeUnknown
    );
    assert!(attention.label().contains("attention"));
    let final_answer = Box::pin(answer(f, &second)).await;
    final_answer.next
}

async fn parked_child(fixture: &FinalFixture, parent: ActivationRequest) -> ActivationRequest {
    use crate::application::workflow::children::WorkflowChildren;
    fixture.p.park_child(parent).await.unwrap().unwrap();
    let row: ScopeRow=sqlx::query_as("SELECT child.id AS run_id,execution.id AS execution_id,job.id AS job_id FROM workflow_runs AS child JOIN workflow_executions AS execution ON execution.company_id=child.company_id AND execution.run_id=child.id JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id WHERE child.company_id=$1 AND child.parent_run_id=$2")
        .bind(parent.company.as_uuid()).bind(parent.run.as_uuid()).fetch_one(fixture.p.pool()).await.unwrap();
    ActivationRequest {
        company: parent.company,
        run: RunId::new(row.run_id),
        execution: ExecutionId::new(row.execution_id),
        job: WorkflowJobId(row.job_id),
    }
}

pub(super) struct ChildReview {
    pub(super) fixture: FinalFixture,
    pub(super) child: ActivationRequest,
}

pub(super) enum ReviewDepth {
    Direct,
    Grandchild,
}

pub(super) async fn child_review(
    s: Intents,
    deadline: chrono::DateTime<Utc>,
    depth: ReviewDepth,
) -> ChildReview {
    let workflow = WorkflowId::new(Uuid::new_v4());
    let mut child: Value =
        serde_json::from_str(&registry::example("decision.human").unwrap().source).unwrap();
    child["steps"]["start"]["with"]["reviewers"] =
        json!({"literal":[{"kind":"user","id":s.owner().user_id()}]});
    child["steps"]["start"]["with"]["deadline"] = json!({"literal":deadline.to_rfc3339()});
    let version = Box::pin(s.fixture.publish_workflow(workflow, child)).await;
    let (workflow, version) = match depth {
        ReviewDepth::Direct => (workflow, version),
        ReviewDepth::Grandchild => {
            let middle = WorkflowId::new(Uuid::new_v4());
            let mut source = final_source("workflow.call");
            source["input_schema"] = json!(true);
            source["steps"]["start"] = json!({"type":"workflow.call","child_workflow_id":workflow,
                "child_version_id":version,"with":{"input":{"literal":{}}},"routes":{"success":"$end"}});
            let version = Box::pin(s.fixture.publish_workflow(middle, source)).await;
            (middle, version)
        }
    };
    let mut parent = final_source("workflow.call");
    parent["steps"]["start"] = json!({"type":"workflow.call","child_workflow_id":workflow,
        "child_version_id":version,"with":{"input":{"literal":{}}},"routes":{"success":"$end"}});
    let fixture = Box::pin(FinalFixture::configured(
        s,
        parent,
        literal_contract("Reviewed final"),
        false,
    ))
    .await;
    let mut child = Box::pin(parked_child(&fixture, fixture.scope)).await;
    if matches!(depth, ReviewDepth::Grandchild) {
        child = Box::pin(parked_child(&fixture, child)).await;
    }
    ChildReview { fixture, child }
}

pub(super) async fn child_questions() -> ChildReview {
    Box::pin(questions_at_depth(
        ReviewDepth::Direct,
        DeliveryPolicy::Deliver,
    ))
    .await
}

async fn questions_at_depth(depth: ReviewDepth, delivery_policy: DeliveryPolicy) -> ChildReview {
    let mut s = Box::pin(classified_intents()).await;
    s.fixture.target.workflow = WorkflowId::new(Uuid::new_v4());
    let mut source = question_source();
    source["workflow_id"] = json!(s.fixture.target.workflow);
    let mut child_workflow = s.fixture.target.workflow;
    let mut version = Box::pin(publish_root(&s, source, FinalPublication::Messaging)).await;
    if matches!(depth, ReviewDepth::Grandchild) {
        let middle = WorkflowId::new(Uuid::new_v4());
        let mut source = final_source("workflow.call");
        source["input_schema"] = json!(true);
        source["steps"]["start"] = json!({"type":"workflow.call","child_workflow_id":child_workflow,
            "child_version_id":version,"with":{"input":{"ref":"/input"}},"routes":{"success":"$end"}});
        version = Box::pin(s.fixture.publish_workflow(middle, source)).await;
        child_workflow = middle;
    }
    let mut parent = final_source("workflow.call");
    parent["steps"]["start"] = json!({"type":"workflow.call","child_workflow_id":child_workflow,
        "child_version_id":version,"with":{"input":{"ref":"/input"}},"routes":{"success":"$end"}});
    let fixture = Box::pin(FinalFixture::from_case(
        s,
        FinalCase {
            source: parent,
            contract: literal_contract("Child answered"),
            policy: None,
            run_as: None,
            publication: FinalPublication::Standard,
            delivery_policy,
        },
    ))
    .await;
    let mut child = Box::pin(parked_child(&fixture, fixture.scope)).await;
    if matches!(depth, ReviewDepth::Grandchild) {
        child = Box::pin(parked_child(&fixture, child)).await;
    }
    ChildReview { fixture, child }
}

#[tokio::test]
async fn channel_reply_views_nested_local_question_wakes_root_at_park_and_answer() {
    let case = Box::pin(questions_at_depth(
        ReviewDepth::Grandchild,
        DeliveryPolicy::InAppOnly,
    ))
    .await;
    let f = &case.fixture;
    let mut parked = listener(f).await;
    let prompt = Box::pin(ask(f, case.child)).await;
    let wakes = question_wakes(&mut parked).await;
    assert!(
        wakes
            .iter()
            .any(|(kind, payload)| kind == "workflow_run_changed"
                && payload["run_id"] == json!(f.scope.run.as_uuid())),
        "A local grandchild prompt has no provider transition to refresh its root: {wakes:?}"
    );
    let current = outcome(f).await;
    assert!(current.prompt.as_ref().unwrap().waiting);
    assert_eq!(current.prompt.as_ref().unwrap().message, prompt.message);
    let ancestors:i64=sqlx::query_scalar("SELECT count(*) FROM workflow_runs AS run JOIN workflow_run_budgets AS budget ON budget.company_id=run.company_id AND budget.run_id=run.id WHERE budget.company_id=$1 AND budget.root_run_id=$2 AND run.id<>$3 AND run.waiting_reason='child_run'")
        .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid()).bind(case.child.run.as_uuid()).fetch_one(f.p.pool()).await.unwrap();
    assert_eq!(ancestors, 2);
    let mut answered = listener(f).await;
    Box::pin(answer(f, &prompt)).await;
    let wakes = question_wakes(&mut answered).await;
    assert!(
        wakes
            .iter()
            .any(|(kind, payload)| kind == "workflow_run_changed"
                && payload["run_id"] == json!(f.scope.run.as_uuid())),
        "{wakes:?}"
    );
    assert!(!outcome(f).await.prompt.as_ref().unwrap().waiting);
    assert!(
        wakes
            .iter()
            .all(|(_, payload)| payload.get("body").is_none()
                && payload.get("respondent_id").is_none()
                && payload.get("wait_id").is_none())
    );
}

async fn question_wakes(events: &mut sqlx::postgres::PgListener) -> Vec<(String, Value)> {
    let mut wakes = Vec::new();
    // Park includes canonical-message, activation and wait transitions as well
    // as the bounded two-run response notification. Fail on an event storm.
    for _ in 0..16 {
        let Ok(result) = tokio::time::timeout(Duration::from_millis(300), events.recv()).await
        else {
            return wakes;
        };
        let event = result.unwrap();
        wakes.push((
            event.channel().to_owned(),
            serde_json::from_str(event.payload()).unwrap(),
        ));
    }
    panic!("A single question transition exhausted its 16-event test budget: {wakes:?}")
}

pub(super) async fn decision_wakes(
    events: &mut sqlx::postgres::PgListener,
) -> Vec<(String, Value)> {
    let mut wakes = Vec::new();
    for _ in 0..4 {
        let Ok(result) = tokio::time::timeout(Duration::from_millis(300), events.recv()).await
        else {
            break;
        };
        let event = result.unwrap();
        wakes.push((
            event.channel().to_owned(),
            serde_json::from_str::<Value>(event.payload()).unwrap(),
        ));
    }
    wakes
}
