//! Real knowledge, two human token decisions, then a genuine CC ingress before resume.
use super::*;
use crate::application::workflow::decisions::{
    DecisionCommandKey, DecisionId, SubmitDecision, WorkflowDecisions,
};

async fn decision(j: &SalesJourney, count: i64) -> DecisionId {
    Box::pin(j.d.run_until("actual token decision", async || {
        j.d.count("SELECT count(*) FROM workflow_decisions WHERE company_id=$1").await == count
            && j.d.count("SELECT count(*) FROM workflow_decisions WHERE company_id=$1 AND submitted_at IS NULL").await == 1
    })).await;
    DecisionId(
        sqlx::query_scalar(
            "SELECT id FROM workflow_decisions WHERE company_id=$1 AND submitted_at IS NULL",
        )
        .bind(j.d.fixture.target.company.as_uuid())
        .fetch_one(j.d.p().pool())
        .await
        .unwrap(),
    )
}

async fn approve(j: &SalesJourney, id: DecisionId) {
    let company = j.d.fixture.target.company;
    let actor = j.d.fixture.target.actor;
    let view = j.d.p().read_decision(company, actor, id).await.unwrap();
    j.d.p()
        .submit_decision(SubmitDecision {
            company,
            actor,
            decision: id,
            command_key: DecisionCommandKey::parse(format!("p07-approve-{}", id.0)).unwrap(),
            expected_revision: view.revision,
            expected_artifact: view.artifact_digest,
            choice: "approve".into(),
            feedback: String::new(),
            data: json!({}),
        })
        .await
        .unwrap();
}

async fn history(j: &SalesJourney) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('id',id,'position',position,'checkpoint',checkpoint_id,'preparation',frozen_preparation,'output',output,'state',state) FROM agent_history_exchanges WHERE company_id=$1")
        .bind(j.d.fixture.target.company.as_uuid()).fetch_one(j.d.p().pool()).await.unwrap()
}

#[tokio::test]
async fn agent_history_e2e_real_search_human_wait_and_customer_cc_refuse_resume_without_provider() {
    let mut j = Box::pin(SalesJourney::new(
        vec![turns("unused").remove(0)],
        Audience::Team,
    ))
    .await;
    let member = j.member(AccessLevel::L5).await;
    let mut source = answering_source();
    source["limits"]["root_budget"] = json!({"activations":10,"model_calls":10,"repetitions":10,
        "tokens":{"max_tokens":1000,"reviewers":[{"kind":"principal","id":j.d.fixture.principal(j.d.fixture.target.actor.user_id()).await}]}});
    j.d.bind(source, Some(MessageTrigger::Direct)).await;
    let first =
        j.d.email(email(&j.d, member.email.as_str(), "p07-waiting-search"))
            .await;
    let thread = first.thread.unwrap().id;
    let initial = decision(&j, 1).await;
    assert!(j.d.llm.observed().is_empty());
    approve(&j, initial).await;
    let next = decision(&j, 2).await;
    assert_ne!(initial, next);
    assert_eq!(j.d.llm.observed().len(), 1);
    assert_eq!(j.facts().await["content"], 5);
    assert_eq!(j.facts().await["proofs"], 1);
    let saved = history(&j).await;
    assert_eq!(saved["state"], "active");
    assert!(saved["checkpoint"].is_string() && saved["output"].is_null());
    let mut cc = email(&j.d, member.email.as_str(), "p07-waiting-cc");
    // Keep the channel in Cc: ingest its changed audience without another Direct activation.
    cc.to = CUSTOMER.into();
    cc.cc = Some(j.d.address("support"));
    cc.text = Some("Adding the customer to this conversation.".into());
    cc.headers = Some("Message-ID: <p07-waiting-cc@partner.test>\nIn-Reply-To: <p07-waiting-search@partner.test>\n".into());
    assert_eq!(j.d.email(cc).await.thread.unwrap().id, thread);
    approve(&j, next).await;
    j.drive(1).await;
    let after = history(&j).await;
    for key in ["id", "position", "checkpoint", "preparation"] {
        assert_eq!(after[key], saved[key], "{key}");
    }
    assert_eq!(after["state"], "failed");
    assert!(after["output"].is_null());
    assert!(
        j.d.llm.observed().is_empty(),
        "downgraded resume cannot enter provider"
    );
    assert_eq!(
        j.d.count("SELECT count(*) FROM workflow_runs WHERE company_id=$1")
            .await,
        1
    );
    assert_eq!(j.d.count("SELECT count(*) FROM task_attempts AS attempt JOIN background_tasks AS job ON job.id=attempt.task_id WHERE job.company_id=$1 AND attempt.workflow_failure_code='knowledge.audience_below_content'").await, 1);
    assert_eq!(j.facts().await["reply_levels"], json!([]));
    j.d.knowledge.shutdown().await.unwrap();
    assert_eq!(j.d.llm.finish().await.unwrap(), 1);
}
