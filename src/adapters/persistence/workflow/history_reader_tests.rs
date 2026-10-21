//! 06-D6 A: `context.history` reads exactly the run's admission snapshot, with every
//! message's disclosure boundary as data, internal notes excluded, bounded by `max_tokens`
//! and the run's result byte cap.
use super::admission_history_tests::Conversation;
use super::*;
use crate::application::workflow::{
    activation::{ActivationRequest, WorkflowJobId},
    history::WorkflowHistoryRuntime,
    lease::{ClaimedWorkflow, WorkflowHandlerOutcome, WorkflowLeases, WorkflowWorkerId},
    polling::WorkflowStepKind,
    worker::WorkflowHandler,
};

fn source(max_tokens: u64, max_context_bytes: u64) -> Value {
    let mut source: Value =
        serde_json::from_str(&registry::example("context.history").unwrap().source).unwrap();
    source["steps"]["start"]["with"]["max_tokens"] = json!({"literal": max_tokens});
    source["limits"]["max_context_bytes"] = json!(max_context_bytes);
    source
}

/// One message in the conversation's thread, `days` days back, with its boundary facts.
async fn message(
    f: &AdmissionFixture,
    conversation: &Conversation,
    days: i32,
    (role, audience, entry_kind): (&str, &str, &str),
    body: &str,
) -> Uuid {
    let p = f.persistence();
    let company = f.binding.target.company.as_uuid();
    let message = Uuid::new_v4();
    sqlx::query("INSERT INTO messages (id, company_id, author_principal_id, subject, clean_text_body, direction, role, audience, correlation_id, content_hash) SELECT $1,$2,id,'History',$3,'inbound',$4,$5,$6,$7 FROM principals WHERE company_id=$2 AND user_id=$8")
        .bind(message).bind(company).bind(body).bind(role).bind(audience).bind(Uuid::new_v4()).bind(vec![0_u8; 32])
        .bind(f.binding.target.actor.user_id()).execute(p.pool()).await.unwrap();
    sqlx::query("INSERT INTO thread_messages (id,company_id,channel_id,thread_id,message_id,entry_kind,created_at) VALUES ($1,$2,$3,$4,$5,$6,CURRENT_TIMESTAMP - make_interval(days => $7))")
        .bind(Uuid::new_v4()).bind(company).bind(conversation.channel).bind(conversation.thread)
        .bind(message).bind(entry_kind).bind(days).execute(p.pool()).await.unwrap();
    message
}

/// A private internal note: never agent history.
async fn private_note(f: &AdmissionFixture, conversation: &Conversation) -> Uuid {
    let p = f.persistence();
    let company = f.binding.target.company.as_uuid();
    let note = message(
        f,
        conversation,
        2,
        ("human", "internal_only", "note"),
        "private reviewer remark",
    )
    .await;
    sqlx::query("INSERT INTO internal_notes (id,company_id,channel_id,thread_id,message_id,command_id,command_fingerprint,author_principal_id,provenance) SELECT $1,$2,$3,$4,$5,$6,'fixture',id,'api' FROM principals WHERE company_id=$2 AND user_id=$7")
        .bind(Uuid::new_v4()).bind(company).bind(conversation.channel).bind(conversation.thread)
        .bind(note).bind(Uuid::new_v4()).bind(f.binding.target.actor.user_id()).execute(p.pool()).await.unwrap();
    note
}

/// Admits a run triggered by `conversation.message` (thread association) and claims it.
async fn claimed(f: &AdmissionFixture, conversation: &Conversation, key: &str) -> ClaimedWorkflow {
    let command = f
        .prepare(conversation.request(f, key, conversation.message))
        .await;
    f.persistence().admit(&command).await.unwrap();
    claim(f, &command).await
}

async fn claim(f: &AdmissionFixture, command: &PreparedAdmission) -> ClaimedWorkflow {
    let job = sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id=$1")
        .bind(command.first_execution_id().as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    let scope = ActivationRequest {
        company: command.company_id(),
        run: command.proposed_run_id(),
        execution: command.first_execution_id(),
        job: WorkflowJobId(job),
    };
    f.persistence()
        .claim_io(
            scope,
            WorkflowWorkerId(Uuid::new_v4()),
            crate::application::workflow::lease::LeasePolicy::new(std::time::Duration::from_secs(
                30,
            ))
            .unwrap(),
        )
        .await
        .unwrap()
        .expect("the history job is claimable")
}

async fn history(f: &AdmissionFixture, claim: &ClaimedWorkflow) -> Value {
    let runtime = WorkflowHistoryRuntime::new(f.persistence().clone());
    let kind = WorkflowStepKind("context.history".into());
    let WorkflowHandlerOutcome::Completed(Ok(output)) = runtime.execute(&kind, claim).await else {
        panic!("context.history completes");
    };
    output
}

fn ids(output: &Value) -> Vec<Uuid> {
    output["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| Uuid::parse_str(item["message_id"].as_str().unwrap()).unwrap())
        .collect()
}

#[tokio::test]
async fn workflow_context_history_returns_the_admission_snapshot_with_boundaries() {
    let f =
        AdmissionFixture::with_binding(BindingFixture::from_source(source(131_072, 65_536)).await)
            .await;
    let conversation = Conversation::new(&f).await;
    let first = message(
        &f,
        &conversation,
        4,
        ("human", "external_conversation", "conversation"),
        "first question",
    )
    .await;
    let delegated = message(
        &f,
        &conversation,
        3,
        ("agent", "internal_only", "delegation"),
        "handed to billing",
    )
    .await;
    let note = private_note(&f, &conversation).await;
    let event = message(
        &f,
        &conversation,
        2,
        ("system", "legacy_unclassified", "system_event"),
        "status changed",
    )
    .await;
    let claim = claimed(&f, &conversation, "history").await;
    // A message added after admission is outside the snapshot.
    let late = message(
        &f,
        &conversation,
        0,
        ("human", "external_conversation", "conversation"),
        "after admission",
    )
    .await;
    let output = history(&f, &claim).await;
    assert_eq!(
        ids(&output),
        vec![first, delegated, event, conversation.message],
        "chronological, triggering message included, note and late message absent"
    );
    assert!(!ids(&output).contains(&note) && !ids(&output).contains(&late));
    let items = output["items"].as_array().unwrap();
    let facts = |index: usize| {
        (
            items[index]["role"].clone(),
            items[index]["audience"].clone(),
            items[index]["entry_kind"].clone(),
        )
    };
    assert_eq!(
        facts(0),
        (
            json!("human"),
            json!("external_conversation"),
            json!("conversation")
        )
    );
    assert_eq!(
        facts(1),
        (json!("agent"), json!("internal_only"), json!("delegation"))
    );
    assert_eq!(
        facts(2),
        (
            json!("system"),
            json!("legacy_unclassified"),
            json!("system_event")
        )
    );
    assert_eq!(items[0]["body"], json!("first question"));
    assert_eq!(items[0]["truncated"], json!(false));
    assert!(items[0]["at"].as_str().unwrap() < items[1]["at"].as_str().unwrap());
    assert_eq!(output["omitted"], json!(0));
    let total: usize = items
        .iter()
        .map(|item| serde_json::to_vec(item).unwrap().len())
        .sum();
    assert_eq!(output["token_count"], json!(total));
    // The engine accepts the output as the step's committed result.
    let stored: Vec<u8> = sqlx::query_scalar("SELECT bundle FROM workflow_runs WHERE id=$1")
        .bind(claim.fence.scope.run.as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    let bundle = publication::restore_bundle(
        &stored,
        &crate::adapters::workflow_source::WorkflowSourceDecoder,
    )
    .unwrap();
    assert!(
        bundle
            .compiled()
            .validate_step_output(&claim.activation.step, &output)
            .is_ok()
    );
}

#[tokio::test]
async fn workflow_context_history_is_bounded_by_tokens_and_the_result_cap() {
    let f =
        AdmissionFixture::with_binding(BindingFixture::from_source(source(131_072, 4096)).await)
            .await;
    let conversation = Conversation::new(&f).await;
    for days in [5, 4, 3, 2] {
        message(
            &f,
            &conversation,
            days,
            ("human", "external_conversation", "conversation"),
            &"long ".repeat(300),
        )
        .await;
    }
    let claim = claimed(&f, &conversation, "bounded").await;
    let output = history(&f, &claim).await;
    let size = serde_json::to_vec(&output).unwrap().len();
    assert!(size <= claim.max_result_bytes, "{size}");
    assert!(output["omitted"].as_u64().unwrap() >= 1);
    assert_eq!(
        *ids(&output).last().unwrap(),
        conversation.message,
        "the newest messages are kept"
    );
    assert_eq!(
        ids(&output).len() as u64 + output["omitted"].as_u64().unwrap(),
        5
    );

    let tight =
        AdmissionFixture::with_binding(BindingFixture::from_source(source(1, 65_536)).await).await;
    let only = Conversation::new(&tight).await;
    let claim = claimed(&tight, &only, "tight").await;
    assert_eq!(
        history(&tight, &claim).await,
        json!({"items": [], "token_count": 0, "omitted": 1})
    );
}

#[tokio::test]
async fn workflow_context_history_is_empty_without_a_thread_and_never_crosses_runs() {
    let f =
        AdmissionFixture::with_binding(BindingFixture::from_source(source(131_072, 65_536)).await)
            .await;
    let command = f.prepare(f.request("manual", f.manual())).await;
    f.persistence().admit(&command).await.unwrap();
    let manual = claim(&f, &command).await;
    assert_eq!(
        history(&f, &manual).await,
        json!({"items": [], "token_count": 0, "omitted": 0})
    );
    let one = Conversation::new(&f).await;
    let other = Conversation::new(&f).await;
    let claim = claimed(&f, &other, "other").await;
    let output = history(&f, &claim).await;
    assert_eq!(ids(&output), vec![other.message]);
    assert!(!ids(&output).contains(&one.message));
}
