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
    sqlx::query("INSERT INTO messages (id, company_id, author_principal_id, subject, clean_text_body, direction, role, audience, correlation_id, content_hash, content_exact) SELECT $1,$2,id,'History',$3,'inbound',$4,$5,$6,$7,CASE WHEN $4='human' THEN NULL ELSE true END FROM principals WHERE company_id=$2 AND user_id=$8")
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
        ("system", "internal_only", "system_event"),
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
            json!("internal_only"),
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
    let stored: Vec<u8> = sqlx::query_scalar("SELECT (SELECT stored.bundle FROM workflow_bundles stored WHERE stored.company_id=workflow_runs.company_id AND stored.digest=workflow_runs.bundle_digest) AS bundle FROM workflow_runs WHERE id=$1")
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

#[path = "knowledge_history_tests.rs"]
mod knowledge_history_tests;

#[path = "knowledge_history_race_tests.rs"]
mod knowledge_history_race_tests;

#[path = "knowledge_history_lease_tests.rs"]
mod knowledge_history_lease_tests;

/// `context.history` lowers the reading run's exactness when any returned message is human
/// (conversational input, derived from the role) or was written by an inexact run; exact
/// agent messages keep it (phase 05 E3). The run is admitted on an exact agent message in its
/// own thread, so the snapshot holds no other human row and the expected bit takes both values.
#[tokio::test]
async fn memory_access_level_history_read_lowers_for_conversational_or_inexact_messages() {
    use crate::adapters::persistence::workflow::knowledge::exactness_fixture::{exact, mark_exact};
    async fn stored(
        f: &AdmissionFixture,
        c: &Conversation,
        days: i32,
        role: &str,
        exact: Option<bool>,
    ) -> Uuid {
        let message = Uuid::new_v4();
        let company = f.binding.target.company.as_uuid();
        sqlx::query("INSERT INTO messages (id, company_id, author_principal_id, subject, clean_text_body, direction, role, audience, correlation_id, content_hash, content_exact) SELECT $1,$2,id,'History','a turn','inbound',$3,'external_conversation',$4,$5,$6 FROM principals WHERE company_id=$2 AND user_id=$7")
            .bind(message).bind(company).bind(role).bind(Uuid::new_v4()).bind(vec![0_u8; 32]).bind(exact)
            .bind(f.binding.target.actor.user_id()).execute(f.persistence().pool()).await.unwrap();
        sqlx::query("INSERT INTO thread_messages (id,company_id,channel_id,thread_id,message_id,entry_kind,created_at) VALUES ($1,$2,$3,$4,$5,'conversation',CURRENT_TIMESTAMP - make_interval(days => $6))")
            .bind(Uuid::new_v4()).bind(company).bind(c.channel).bind(c.thread).bind(message).bind(days)
            .execute(f.persistence().pool()).await.unwrap();
        message
    }
    let mut seen = Vec::new();
    for (case, role, stored_exact) in [
        ("agent_exact", "agent", Some(true)),
        ("agent_inexact", "agent", Some(false)),
        ("system_inexact", "system", Some(false)),
        ("human", "human", None),
    ] {
        let f = AdmissionFixture::with_binding(
            BindingFixture::from_source(source(131_072, 65_536)).await,
        )
        .await;
        let base = Conversation::new(&f).await;
        let thread = Uuid::new_v4();
        sqlx::query("INSERT INTO threads (id, company_id, channel_id, subject) VALUES ($1,$2,$3,'Agent only')")
            .bind(thread).bind(f.binding.target.company.as_uuid()).bind(base.channel)
            .execute(f.persistence().pool()).await.unwrap();
        let mut c = Conversation {
            channel: base.channel,
            thread,
            message: Uuid::nil(),
        };
        let earlier = stored(&f, &c, 2, role, stored_exact).await;
        c.message = stored(&f, &c, 1, "agent", Some(true)).await;
        let claim = claimed(&f, &c, case).await;
        let scope = claim.fence.scope;
        mark_exact(f.persistence().pool(), scope.company, scope.run).await;
        let read = ids(&history(&f, &claim).await);
        assert_eq!(
            read.len(),
            2,
            "{case}: only the earlier turn and the agent trigger: {read:?}"
        );
        assert!(
            read.contains(&earlier) && read.contains(&c.message),
            "{case}"
        );
        let kept = exact(f.persistence().pool(), scope.company, scope.run).await;
        assert_eq!(kept, case == "agent_exact", "{case}");
        seen.push(kept);
    }
    assert!(
        seen.contains(&true) && seen.contains(&false),
        "the expectation discriminates"
    );
}
