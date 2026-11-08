use super::*;
use crate::application::workflow::WorkflowActor;
use crate::application::workflow::decisions::{DecisionCommandKey, DecisionId, SubmitDecision};

struct NativeReview {
    d: Deployment,
    binding: Uuid,
    reviewer: WorkflowActor,
}

async fn awaiting_review() -> NativeReview {
    let d = Box::pin(Deployment::new(vec![
        LlmTurn::tool_call(CREATE_AGENT_CHANNEL, spec("reviewed", ReplyMode::Reviewed)),
        LlmTurn::text(r#"{"body":"Created"}"#),
        LlmTurn::text(r#"{"body":"Unapproved draft"}"#),
    ]))
    .await;
    let builder_binding = Box::pin(builder(&d)).await;
    let (admin, email) = person(&d, "admin").await;
    write(
        &d,
        &email,
        "builder",
        "Create a reviewed assistant",
        IngressOrigin::TrustedApplication,
    )
    .await;
    Box::pin(d.run_until("builder response published", async || {
        replied(&d, builder_binding).await == 1
    }))
    .await;
    let created = receipts(&d, builder_binding).await.remove(0);
    let binding = Uuid::parse_str(created["binding_id"].as_str().unwrap()).unwrap();
    register_scripted_agent_base_url(
        Uuid::parse_str(created["agent"]["id"].as_str().unwrap()).unwrap(),
        &d.llm.base_url,
    );
    write(
        &d,
        &email,
        "reviewed",
        "A private business question",
        IngressOrigin::TrustedApplication,
    )
    .await;
    Box::pin(d.run_until("native private review",async || {
        d.count(&format!("SELECT count(*) FROM workflow_runs WHERE company_id=$1 AND binding_id='{binding}' AND waiting_reason='decision'")).await==1
    })).await;
    NativeReview {
        d,
        binding,
        reviewer: WorkflowActor::authenticated(admin).unwrap(),
    }
}

#[tokio::test]
async fn channel_reply_authoring_native_real_review_closes_reject_and_publishes_only_edited_approval()
 {
    for choice in ["approve", "reject"] {
        let f = Box::pin(awaiting_review()).await;
        let company = f.d.fixture.target.company;
        let decision: Uuid = sqlx::query_scalar("SELECT decision.id FROM workflow_decisions AS decision JOIN workflow_runs AS run ON run.company_id=decision.company_id AND run.id=decision.run_id WHERE run.binding_id=$1")
            .bind(f.binding).fetch_one(f.d.p().pool()).await.unwrap();
        let service = f.d.engine.services();
        let before = service
            .decisions()
            .read_decision(company, f.reviewer, DecisionId(decision))
            .await
            .unwrap();
        service
            .decisions()
            .submit_decision(SubmitDecision {
                company,
                actor: f.reviewer,
                decision: before.decision,
                command_key: DecisionCommandKey::parse(choice).unwrap(),
                expected_revision: before.revision,
                expected_artifact: before.artifact_digest.clone(),
                choice: choice.into(),
                feedback: "Reviewed privately".into(),
                data: json!({"body":"Exact edited answer"}),
            })
            .await
            .unwrap();
        Box::pin(f.d.run_until("native terminal result",async || {
            f.d.count(&format!("SELECT count(*) FROM channel_reply_decisions AS final JOIN workflow_runs AS run ON run.company_id=final.company_id AND run.id=final.run_id WHERE run.company_id=$1 AND run.binding_id='{}' AND final.state='{}'",f.binding,if choice=="approve" {"published"} else {"suppressed"})).await==1
        })).await;
        let closed = service
            .decisions()
            .read_decision(company, f.reviewer, DecisionId(decision))
            .await
            .unwrap();
        assert_eq!(closed.state, "completed");
        assert_eq!(closed.submission.unwrap().choice, choice);
        let (state,output,message): (String,Value,Option<Uuid>) = sqlx::query_as("SELECT run.state,execution.committed_output,final.message_id FROM workflow_runs AS run JOIN workflow_executions AS execution ON execution.company_id=run.company_id AND execution.id=run.terminal_execution_id JOIN channel_reply_decisions AS final ON final.company_id=run.company_id AND final.run_id=run.id WHERE run.binding_id=$1")
            .bind(f.binding).fetch_one(f.d.p().pool()).await.unwrap();
        assert_eq!(state, "succeeded");
        assert_eq!(
            reply_count(&f.d).await,
            if choice == "approve" { 2 } else { 1 }
        );
        if choice == "approve" {
            assert_eq!(output["body"], "Exact edited answer");
            let body: String =
                sqlx::query_scalar("SELECT clean_text_body FROM messages WHERE id=$1")
                    .bind(message.unwrap())
                    .fetch_one(f.d.p().pool())
                    .await
                    .unwrap();
            assert_eq!(body, "Exact edited answer");
        } else {
            assert!(message.is_none());
            assert_eq!(output["review"]["choice"], "reject");
        }
        assert_eq!(f.d.llm.finish().await.unwrap(), 3);
    }
}
