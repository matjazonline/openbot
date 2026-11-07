use super::*;
use crate::adapters::persistence::workflow::tests::approval_test_support;
use crate::application::workflow::DeliveryPolicy;
use crate::application::workflow::{activation::*, batch::*, channel_reply::*, decisions::*};
#[path = "channel_reply_intent_schema_tests.rs"]
mod intent_schema_tests;
#[path = "channel_reply_completion_owner_tests.rs"]
mod owner_tests;
#[path = "channel_reply_completion_policy_runtime_tests.rs"]
mod policy_runtime_tests;
#[path = "channel_reply_publication_tests.rs"]
mod publication_tests;
#[path = "channel_question_final_tests.rs"]
mod question_final_tests;
#[path = "channel_reply_completion_slot_tests.rs"]
mod slot_tests;

struct FinalFixture {
    s: Intents,
    p: PostgresPersistence,
    scope: ActivationRequest,
    intent: Uuid,
}
#[derive(sqlx::FromRow)]
struct ScopeRow {
    run_id: Uuid,
    execution_id: Uuid,
    job_id: Uuid,
}

struct FinalCase {
    source: Value,
    contract: ChannelReplyContract,
    policy: Option<ReplyApprovalPolicy>,
    run_as: Option<Uuid>,
    publication: FinalPublication,
    delivery_policy: DeliveryPolicy,
}
enum FinalPublication {
    Standard,
    Messaging,
}
enum FinalIngress {
    Email,
    Internal,
}

fn final_source(kind: &str) -> Value {
    let mut source: Value = serde_json::from_str(&registry::example(kind).unwrap().source).unwrap();
    source["input_schema"] = crate::application::workflow::triggers::MessageTriggerInput::schema();
    source["output_schema"] = json!(true);
    source["steps"]["start"]["with"] =
        json!({"value":{"literal":{"result":42}},"output_schema":{"literal":true}});
    source
}

impl FinalFixture {
    async fn command(&self, choice: &str) -> SubmitDecision {
        let decision: Uuid=sqlx::query_scalar("SELECT id FROM workflow_decisions WHERE company_id=$1 AND execution_id=$2 AND reply_proposal_id IS NOT NULL")
            .bind(self.scope.company.as_uuid()).bind(self.scope.execution.as_uuid()).fetch_one(self.p.pool()).await.unwrap();
        self.decision_command(DecisionId(decision), choice).await
    }

    async fn decision_command(&self, decision: DecisionId, choice: &str) -> SubmitDecision {
        let view = self
            .p
            .read_decision(self.scope.company, self.s.owner(), decision)
            .await
            .unwrap();
        SubmitDecision {
            company: self.scope.company,
            actor: self.s.owner(),
            decision,
            command_key: DecisionCommandKey::parse(format!("final-{choice}")).unwrap(),
            expected_revision: view.revision,
            expected_artifact: view.artifact_digest,
            choice: choice.into(),
            feedback: String::new(),
            data: json!({}),
        }
    }
    async fn new(kind: &str, approval: bool) -> Self {
        let source = final_source(kind);
        let s = Intents::new().await;
        Box::pin(Self::configured(
            s,
            source,
            literal_contract("Done"),
            approval,
        ))
        .await
    }

    async fn configured(
        s: Intents,
        source: Value,
        contract: ChannelReplyContract,
        approval: bool,
    ) -> Self {
        let policy =
            approval.then(|| ReplyApprovalPolicy::new(vec![s.owner().user_id()], 60).unwrap());
        Box::pin(Self::from_case(
            s,
            FinalCase {
                source,
                contract,
                policy,
                run_as: None,
                publication: FinalPublication::Standard,
                delivery_policy: DeliveryPolicy::Deliver,
            },
        ))
        .await
    }

    async fn from_case(s: Intents, case: FinalCase) -> Self {
        // Keep fixture construction shallow on the stock test stack.
        Box::pin(Self::from_ingress(s, case, FinalIngress::Email)).await
    }

    async fn from_ingress(mut s: Intents, case: FinalCase, ingress: FinalIngress) -> Self {
        let FinalCase {
            mut source,
            contract,
            policy,
            run_as,
            publication,
            delivery_policy,
        } = case;
        // The shared legacy fixture disables this guard for unrelated corruption tests.
        // These protocol tests exercise the production schema, including immutable roots.
        sqlx::query("ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable")
            .execute(s.pool())
            .await
            .unwrap();
        source["input_schema"] =
            crate::application::workflow::triggers::MessageTriggerInput::schema();
        s.fixture.target.workflow = WorkflowId::new(Uuid::new_v4());
        source["workflow_id"] = json!(s.fixture.target.workflow.to_string());
        // Box publication/restore so fixture setup retains the stock test stack.
        s.version = Box::pin(publish_root(&s, source, publication)).await;
        let binding = s.binding(MessageTrigger::Any, run_as).await;
        let mut request = selection(&s, binding, None).await;
        request.responder.as_mut().unwrap().contract = contract;
        request.approval_policy = policy;
        s.p().configure_channel_reply(request).await.unwrap();
        let intent = match ingress {
            FinalIngress::Email => s.intent_with(TO, delivery_policy.as_str()).await,
            FinalIngress::Internal => {
                let mut message = inbound(
                    s.thread,
                    "<internal@partner.test>",
                    &[],
                    "Internal question",
                );
                message.audience = crate::entities::message::MessageAudience::InternalOnly;
                message.correlation =
                    crate::application::ports::thread::MessageCorrelation::Internal;
                let source = s.p().create_message(&message).await.unwrap().canonical_id;
                s.intent_from_message(TO, delivery_policy.as_str(), source)
                    .await
            }
        };
        s.drain(&s.admissions(admission(s.p()))).await;
        assert_eq!(
            shape(&s, intent).await,
            (1, 1),
            "admission outcomes: {:?}",
            s.outcomes(intent).await
        );
        let row: ScopeRow=sqlx::query_as("SELECT owned.run_id,execution.id AS execution_id,job.id AS job_id \
            FROM channel_reply_runs AS owned JOIN workflow_executions AS execution ON execution.company_id=owned.company_id AND execution.run_id=owned.run_id \
            JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id WHERE owned.company_id=$1 AND owned.intent_id=$2")
            .bind(s.fixture.target.company.as_uuid()).bind(intent).fetch_one(s.pool()).await.unwrap();
        let p = s
            .p()
            .clone()
            .with_channel_reply_notices(approval_test_support::notices(s.p()));
        let scope = ActivationRequest {
            company: s.fixture.target.company,
            run: RunId::new(row.run_id),
            execution: ExecutionId::new(row.execution_id),
            job: WorkflowJobId(row.job_id),
        };
        Self {
            s,
            p,
            scope,
            intent,
        }
    }
    async fn advance(&self) -> BatchResult {
        self.p
            .advance_pure(
                self.scope,
                BatchBudget::new(4, Duration::from_millis(100)).unwrap(),
            )
            .await
            .unwrap()
    }
    async fn state(&self) -> Value {
        sqlx::query_scalar("SELECT jsonb_build_object('run',run.state,'completed',execution.completed_at IS NOT NULL,'output',execution.committed_output, \
            'proposals',(SELECT count(*) FROM channel_reply_proposals WHERE company_id=$1 AND run_id=$2), \
            'decision',(SELECT state FROM channel_reply_decisions WHERE company_id=$1 AND run_id=$2)) \
            FROM workflow_runs AS run JOIN workflow_executions AS execution ON execution.company_id=run.company_id AND execution.run_id=run.id \
            WHERE run.company_id=$1 AND run.id=$2 AND execution.id=$3")
            .bind(self.scope.company.as_uuid()).bind(self.scope.run.as_uuid()).bind(self.scope.execution.as_uuid()).fetch_one(self.p.pool()).await.unwrap()
    }
}

async fn publish_root(s: &Intents, source: Value, publication: FinalPublication) -> VersionId {
    match publication {
        FinalPublication::Standard => {
            s.fixture
                .publish_workflow(s.fixture.target.workflow, source)
                .await
        }
        FinalPublication::Messaging => {
            let directory =
                crate::adapters::persistence::workflow::message::MessagePublicationDirectory {
                    persistence: s.p().clone(),
                };
            let definitions =
                DefinitionService::new(Preflight, s.p().clone(), WorkflowSourceDecoder, directory);
            let version = VersionId::new(Uuid::new_v4());
            s.fixture
                .publish_source(
                    &definitions,
                    s.fixture.target.workflow,
                    source.to_string(),
                    version,
                )
                .await
                .unwrap();
            version
        }
    }
}

#[tokio::test]
async fn channel_reply_completion_automatic_and_suppressed_are_atomic() {
    for kind in ["data.map", "channel.no_reply"] {
        let f = FinalFixture::new(kind, false).await;
        let done = f.advance().await;
        assert_eq!(done.disposition, BatchDisposition::Completed);
        let state = f.state().await;
        assert_eq!(state["run"], "succeeded");
        assert_eq!(state["completed"], true);
        assert_eq!(state["output"], json!({"result":42}));
        assert_eq!(state["proposals"], i64::from(kind == "data.map"));
        assert_eq!(
            state["decision"],
            if kind == "data.map" {
                "pending"
            } else {
                "suppressed"
            }
        );
        f.advance().await;
        assert_eq!(f.state().await, state);
    }
}

#[tokio::test]
async fn channel_reply_completion_approval_resumes_exact_saved_output() {
    let f = FinalFixture::new("data.map", true).await;
    assert_eq!(f.advance().await.disposition, BatchDisposition::Deferred);
    assert_eq!(
        f.state().await,
        json!({"run":"waiting","completed":false,"output":null,"proposals":1,"decision":null})
    );
    let command = approval_test_support::command(&f.p, f.scope, f.s.owner(), "approve").await;
    f.p.submit_decision(command).await.unwrap();
    let resumed = f.p.resume_finalization(f.scope).await.unwrap();
    let FinalizationProgress::Completed(step) = resumed else {
        panic!("saved completion was not resumed")
    };
    assert_eq!(step.output, json!({"result":42}));
    assert_eq!(f.state().await["decision"], "pending");
    let attempts: i64 = sqlx::query_scalar("SELECT count(*) FROM task_attempts WHERE task_id=$1")
        .bind(f.scope.job.0)
        .fetch_one(f.p.pool())
        .await
        .unwrap();
    assert_eq!(
        attempts, 0,
        "a pure completion must never manufacture an I/O attempt"
    );
}

#[tokio::test]
async fn channel_reply_completion_competing_finalizers_create_one_obligation() {
    let f = FinalFixture::new("data.map", false).await;
    let barrier = tokio::sync::Barrier::new(2);
    let complete = || async {
        barrier.wait().await;
        f.advance().await
    };
    let (a, b) = tokio::join!(complete(), complete());
    assert!(matches!(
        (&a.disposition, &b.disposition),
        (BatchDisposition::Completed, BatchDisposition::Replay)
            | (BatchDisposition::Replay, BatchDisposition::Completed)
    ));
    assert_eq!(f.state().await["proposals"], 1);
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM channel_reply_decisions WHERE company_id=$1 AND intent_id=$2",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.intent)
    .fetch_one(f.p.pool())
    .await
    .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn channel_reply_completion_missing_notice_configuration_cannot_commit_success() {
    let f = FinalFixture::new("data.map", true).await;
    let result =
        f.s.p()
            .advance_pure(
                f.scope,
                BatchBudget::new(4, Duration::from_millis(100)).unwrap(),
            )
            .await
            .unwrap();
    assert_eq!(result.disposition, BatchDisposition::Failed);
    assert_eq!(f.state().await["run"], "failed");
    assert_eq!(f.state().await["proposals"], 0);
    assert_eq!(f.state().await["decision"], Value::Null);
}

#[tokio::test]
async fn channel_reply_completion_policy_change_after_approval_fails_saved_step() {
    let f = FinalFixture::new("data.map", true).await;
    f.advance().await;
    let command = approval_test_support::command(&f.p, f.scope, f.s.owner(), "approve").await;
    f.p.submit_decision(command).await.unwrap();
    let selected =
        f.s.p()
            .channel_reply_settings(
                f.scope.company,
                RelatedChannelId::new(f.s.channel),
                f.s.owner(),
            )
            .await
            .unwrap()
            .unwrap();
    let binding = selected.responder.unwrap().binding;
    // Loosening a future policy cannot authorize this already-frozen proposal.
    f.s.p()
        .configure_channel_reply(selection(&f.s, binding, Some(selected.revision)).await)
        .await
        .unwrap();
    assert!(matches!(
        f.p.resume_finalization(f.scope).await.unwrap(),
        FinalizationProgress::Refused
    ));
    assert_eq!(f.state().await["run"], "failed");
    assert_eq!(f.state().await["decision"], Value::Null);
    assert_eq!(f.state().await["proposals"], 1);
}

#[tokio::test]
async fn channel_reply_completion_rejection_uses_failure_without_publication() {
    let f = FinalFixture::new("data.map", true).await;
    f.advance().await;
    let command = approval_test_support::command(&f.p, f.scope, f.s.owner(), "reject").await;
    f.p.submit_decision(command).await.unwrap();
    assert_eq!(f.state().await["run"], "failed");
    assert_eq!(f.state().await["decision"], Value::Null);
    assert!(matches!(
        f.p.resume_finalization(f.scope).await.unwrap(),
        FinalizationProgress::Refused
    ));
}

#[tokio::test]
async fn channel_reply_completion_contract_edits_cannot_replace_approved_content() {
    let f = FinalFixture::new("data.map", true).await;
    f.advance().await;
    let selected =
        f.p.channel_reply_settings(
            f.scope.company,
            RelatedChannelId::new(f.s.channel),
            f.s.owner(),
        )
        .await
        .unwrap()
        .unwrap();
    let mut changed = selection(
        &f.s,
        selected.responder.unwrap().binding,
        Some(selected.revision),
    )
    .await;
    changed.approval_policy = selected.approval_policy;
    changed.responder.as_mut().unwrap().contract = literal_contract("Replacement");
    f.p.configure_channel_reply(changed).await.unwrap();
    let command = approval_test_support::command(&f.p, f.scope, f.s.owner(), "approve").await;
    f.p.submit_decision(command).await.unwrap();
    assert!(matches!(
        f.p.resume_finalization(f.scope).await.unwrap(),
        FinalizationProgress::Completed(_)
    ));
    let content: Value = sqlx::query_scalar(
        "SELECT content FROM channel_reply_proposals WHERE company_id=$1 AND run_id=$2",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.scope.run.as_uuid())
    .fetch_one(f.p.pool())
    .await
    .unwrap();
    assert_eq!(content, json!({"subject":"Status","body":"Done"}));
}

#[tokio::test]
async fn channel_reply_completion_commit_failure_and_lost_ack_preserve_atomic_decision() {
    let f = FinalFixture::new("data.map", false).await;
    sqlx::raw_sql("CREATE FUNCTION reject_final_decision() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected final decision failure'; END $$; \
        CREATE TRIGGER reject_final_decision BEFORE INSERT ON channel_reply_decisions FOR EACH ROW EXECUTE FUNCTION reject_final_decision();")
        .execute(f.p.pool()).await.unwrap();
    assert!(
        f.p.advance_pure(
            f.scope,
            BatchBudget::new(4, Duration::from_millis(100)).unwrap()
        )
        .await
        .is_err()
    );
    assert_eq!(f.state().await["proposals"], 0);
    assert_eq!(f.state().await["completed"], false);
    sqlx::raw_sql("DROP TRIGGER reject_final_decision ON channel_reply_decisions; DROP FUNCTION reject_final_decision();").execute(f.p.pool()).await.unwrap();
    let mut tx = f.p.pool().begin().await.unwrap();
    crate::adapters::persistence::workflow::batch::advance_on(
        &mut tx,
        f.scope,
        BatchBudget::new(4, Duration::from_millis(100)).unwrap(),
        f.p.channel_reply_notices(),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap(); // Discard the receipt as a caller that lost the commit acknowledgement.
    let stored = f.state().await;
    assert_eq!(f.advance().await.disposition, BatchDisposition::Replay);
    assert_eq!(f.state().await, stored);
}

#[tokio::test]
async fn channel_reply_completion_invalid_projection_fails_or_uses_original_error_route() {
    for error_route in [None, Some("recover")] {
        let mut source: Value =
            serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
        source["output_schema"] = json!({"type":"object","properties":{"body":{"type":"string"}},"required":["body"],"additionalProperties":false});
        source["steps"]["start"]["with"] =
            json!({"value":{"literal":{"body":""}},"output_schema":{"literal":true}});
        if let Some(route) = error_route {
            source["steps"]["start"]["routes"]["error"] = json!(route);
            source["steps"]["recover"] = json!({"type":"channel.no_reply","with":{"value":{"literal":{"body":""}},"output_schema":{"literal":true}},"routes":{"success":"$end"}});
        }
        let contract = ChannelReplyContract {
            format_version: 1,
            subject: ReplyText::Literal {
                literal: "Status".into(),
            },
            body: ReplyText::Reference {
                r#ref: ReplyReference::try_from("/input/body".to_owned()).unwrap(),
            },
        };
        let mut f = FinalFixture::configured(Intents::new().await, source, contract, false).await;
        assert_eq!(f.advance().await.disposition, BatchDisposition::Failed);
        assert_eq!(f.state().await["proposals"], 0);
        assert_eq!(f.state().await["decision"], Value::Null);
        if error_route.is_some() {
            let next: ScopeRow=sqlx::query_as("SELECT run_id,id AS execution_id,(SELECT id FROM background_tasks WHERE workflow_execution_id=execution.id) AS job_id \
                FROM workflow_executions AS execution WHERE company_id=$1 AND run_id=$2 AND activation=2")
                .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid()).fetch_one(f.p.pool()).await.unwrap();
            f.scope.execution = ExecutionId::new(next.execution_id);
            f.scope.job = WorkflowJobId(next.job_id);
            assert_eq!(
                f.advance().await.response_disposition,
                Some(ResponseDisposition::Suppressed)
            );
        } else {
            assert_eq!(f.state().await["run"], "failed");
        }
    }
}
