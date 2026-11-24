//! agent-workflow-call p05 B1 (gate AWC-P05-D1 r2): desk discovery in the `channel.call` help,
//! prepared and frozen with the agent step.
use super::channel_call_support::*;
use super::workflow_tool_support::*;
use super::*;
use crate::adapters::persistence::workflow::channel_call_target::CallStart;
use crate::application::workflow::agent::{AgentPreparation, AgentToolTarget};
use crate::application::workflow::channel_call_listing::DeskListing;
use crate::application::workflow::lease::ClaimedWorkflow;
use serde_json::{Value, json};

/// The desk list the claim's agent step is prepared with.
async fn listing(s: &CallSetup, claim: &ClaimedWorkflow) -> DeskListing {
    let launch = s
        .p()
        .prepare_agent(claim, lease())
        .await
        .unwrap()
        .expect_ready();
    launch
        .prepared
        .tools()
        .iter()
        .find_map(|tool| match &tool.target {
            AgentToolTarget::ChannelCall { desks } => Some(desks.clone()),
            _ => None,
        })
        .expect("the agent may call desks")
}

fn listed_ids(listing: &DeskListing) -> Vec<Uuid> {
    match listing {
        DeskListing::Listed { desks, .. } => desks.iter().map(|desk| desk.id).collect(),
        other => panic!("expected a list, got {other:?}"),
    }
}

async fn run_level(s: &CallSetup, run: RunId) -> i16 {
    sqlx::query_scalar("SELECT content_level::smallint FROM workflow_runs WHERE id=$1")
        .bind(run.as_uuid())
        .fetch_one(s.pool())
        .await
        .unwrap()
}

async fn set(s: &CallSetup, sql: &str, desk: Uuid) {
    sqlx::query(sql).bind(desk).execute(s.pool()).await.unwrap();
}

/// A public, enabled desk in its own unit with no responder selected.
async fn bare_desk(s: &CallSetup, name: &str) -> Uuid {
    let desk = super::support::new_channel(&s.tool.fixture, name).await;
    let unit = s
        .p()
        .test_unit(s.company().as_uuid(), &format!("{name}-unit"))
        .await;
    sqlx::query(
        "UPDATE channels SET enabled=true, access_mode='public', department_id=$2 WHERE id=$1",
    )
    .bind(desk)
    .bind(unit)
    .execute(s.pool())
    .await
    .unwrap();
    desk
}

/// C3: only public desks that launch would admit are listed, judged by the launch readers; a
/// members-only desk the agent may call is still not listed (U8).
#[tokio::test]
async fn channel_call_authoring_discovery_lists_callable_public_desks() {
    let s = CallSetup::new().await;
    let (home, _) = s.add_desk("home", s.version, json!({})).await;
    let (granted, _) = s.add_desk("granted", s.version, json!({})).await;
    set(
        &s,
        "UPDATE channels SET access_mode='members' WHERE id=$1",
        granted,
    )
    .await;
    s.participate_on(granted, true).await;
    let (members, _) = s.add_desk("members", s.version, json!({})).await;
    set(
        &s,
        "UPDATE channels SET access_mode='members' WHERE id=$1",
        members,
    )
    .await;
    let (disabled, _) = s.add_desk("disabled", s.version, json!({})).await;
    set(
        &s,
        "UPDATE channels SET enabled=false WHERE id=$1",
        disabled,
    )
    .await;
    let (shelved, _) = s.add_desk("shelved", s.version, json!({})).await;
    let shelved_unit: Uuid = sqlx::query_scalar("SELECT department_id FROM channels WHERE id=$1")
        .bind(shelved)
        .fetch_one(s.pool())
        .await
        .unwrap();
    crate::adapters::persistence::test_support::department_use_cases(s.p())
        .archive_department(
            s.owner().user_id(),
            s.company().as_uuid(),
            crate::entities::department::DepartmentId::new(shelved_unit),
            crate::application::ports::department::OpenWork::Refuse,
        )
        .await
        .unwrap();
    let silent = bare_desk(&s, "silent").await;
    // The caller runs on its own desk, which its ancestry excludes.
    let claim = s.claim_from(home, "discover").await;
    let listed = listing(&s, &claim).await;
    assert_eq!(listed_ids(&listed), vec![s.desk]);
    let agent_principal: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND agent_id=$2")
            .bind(s.company().as_uuid())
            .bind(s.tool.agent)
            .fetch_one(s.pool())
            .await
            .unwrap();
    assert!(
        !listed_ids(&listed).contains(&agent_principal),
        "never an inbox"
    );
    // Each launch takes a live claim of its own; cancelling each one keeps the company under its
    // dispatch cap.
    s.tool.cancel(claim.fence.scope, "discover-done").await;
    // Listed equals launched, desk by desk, except the granted members desk (U8).
    for (index, desk) in [s.desk, home, granted, members, disabled, shelved, silent]
        .into_iter()
        .enumerate()
    {
        let claim = s.claim_from(home, &format!("launch-{index}")).await;
        let arguments = json!({"channel_id": desk.to_string(), "content": {"body": "Hello"}});
        let launched = matches!(
            s.start(&claim, &arguments).await.unwrap(),
            CallStart::Started { .. }
        );
        let listed = listed_ids(&listed).contains(&desk);
        if desk == granted {
            assert!(launched && !listed, "callable by id, never listed (U8)");
        } else {
            assert_eq!(launched, listed, "desk {index}");
        }
        s.tool
            .cancel(claim.fence.scope, &format!("launch-{index}-done"))
            .await;
    }
}

/// C3a, C3b: nothing is listed while the run can answer an external correspondent; a call
/// child counts as internal; preparing the list raises no knowledge level.
#[tokio::test]
async fn channel_call_authoring_discovery_omitted_for_external() {
    let s = CallSetup::new().await;
    // An internal thread lists desks.
    let (internal, _) = s.claim_in_thread("discover-internal").await;
    let before = run_level(&s, internal.fence.scope.run).await;
    assert_eq!(listed_ids(&listing(&s, &internal).await), vec![s.desk]);
    assert_eq!(
        run_level(&s, internal.fence.scope.run).await,
        before,
        "the list adds no level"
    );
    // A run admitted manually (no trigger message) on a thread holding a customer's message lists
    // nothing: this is the manual rerun on a customer thread (gate F9, source 2).
    let (external, thread) = s.claim_in_thread("discover-external").await;
    let message = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO messages (id, company_id, author_principal_id, subject, clean_text_body, direction, role, correlation_id, content_hash, audience) \
         SELECT $1,$2,id,'Help','Customer text','inbound','human',$3,$4,'external_conversation' FROM principals WHERE company_id=$2 AND user_id=$5",
    )
    .bind(message)
    .bind(s.company().as_uuid())
    .bind(Uuid::new_v4())
    .bind(vec![0_u8; 32])
    .bind(s.owner().user_id())
    .execute(s.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO thread_messages (id,company_id,channel_id,thread_id,message_id,entry_kind) \
         SELECT $1,company_id,channel_id,id,$3,'conversation' FROM threads WHERE id=$2",
    )
    .bind(Uuid::new_v4())
    .bind(thread)
    .bind(message)
    .execute(s.pool())
    .await
    .unwrap();
    let omitted = listing(&s, &external).await;
    assert_eq!(omitted, DeskListing::External);
    let help = omitted.render();
    assert!(help.contains("unavailable while answering an external correspondent"));
    // A call from that external run reaches an agent desk, whose step lists desks: the call
    // child answers the calling agent, not the customer.
    let binding = s
        .responder_on(s.desk, s.tool.parent, s.owner().user_id(), json!({}))
        .await;
    s.select(
        binding,
        Some(
            crate::application::workflow::channel_reply::ReplySettingsRevision::try_from(1)
                .unwrap(),
        ),
    )
    .await
    .unwrap();
    assert!(matches!(
        s.call(&external, &s.arguments("Please check"))
            .await
            .unwrap(),
        crate::application::workflow::agent::AgentToolObservation::Parked(_)
    ));
    let child = s.tool.child_of(external.fence.scope).await;
    let claimed = s
        .p()
        .claim_io(child, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        listing(&s, &claimed).await,
        DeskListing::Listed { .. }
    ));
}

/// C4: the list is frozen with the prepared step: a desk added later is absent on resume; a desk
/// disabled later is still listed but refused at launch.
#[tokio::test]
async fn channel_call_authoring_discovery_frozen() {
    let s = CallSetup::new().await;
    let claim = s.claim("discover-frozen").await;
    let first = listing(&s, &claim).await;
    assert_eq!(listed_ids(&first), vec![s.desk]);
    s.add_desk("later", s.version, json!({})).await;
    set(&s, "UPDATE channels SET enabled=false WHERE id=$1", s.desk).await;
    assert_eq!(
        listing(&s, &claim).await,
        first,
        "the resume replays the frozen list"
    );
    let refused = s.start(&claim, &s.arguments("Still there?")).await.unwrap();
    assert!(!matches!(refused, CallStart::Started { .. }));
    // A new step lists afresh.
    let fresh = s.claim("discover-fresh").await;
    let later = listed_ids(&listing(&s, &fresh).await);
    assert_eq!(later.len(), 1);
    assert_ne!(later, vec![s.desk]);
}

/// C2: an agent without the `channel.call` binding is prepared with no such tool.
#[tokio::test]
async fn channel_call_authoring_ungranted_never_sees() {
    let setup = Box::pin(ToolSetup::new(echo_child())).await;
    let claim = setup.claim("ungranted").await;
    let launch = setup
        .persistence()
        .prepare_agent(&claim, lease())
        .await
        .unwrap()
        .expect_ready();
    assert!(!launch.prepared.calls_desks());
    let declared: Vec<Value> = launch
        .prepared
        .tools()
        .iter()
        .map(|tool| json!(tool.declaration().name.as_str()))
        .collect();
    assert!(!declared.contains(&json!("channel_call")), "{declared:?}");
}

/// A customer message, filed on a fresh thread of `channel` when one is given.
async fn customer_message(s: &CallSetup, channel: Option<Uuid>) -> (Uuid, Option<Uuid>) {
    let message = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO messages (id, company_id, author_principal_id, subject, clean_text_body, direction, role, correlation_id, content_hash, audience) \
         SELECT $1,$2,id,'Help','Customer text','inbound','human',$3,$4,'external_conversation' FROM principals WHERE company_id=$2 AND user_id=$5",
    )
    .bind(message)
    .bind(s.company().as_uuid())
    .bind(Uuid::new_v4())
    .bind(vec![0_u8; 32])
    .bind(s.owner().user_id())
    .execute(s.pool())
    .await
    .unwrap();
    let Some(channel) = channel else {
        return (message, None);
    };
    let thread = Uuid::new_v4();
    sqlx::query("INSERT INTO threads (id, company_id, channel_id, subject) VALUES ($1,$2,$3,'Customer asks')")
        .bind(thread)
        .bind(s.company().as_uuid())
        .bind(channel)
        .execute(s.pool())
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO thread_messages (id,company_id,channel_id,thread_id,message_id,entry_kind) \
         VALUES ($1,$2,$3,$4,$5,'conversation')",
    )
    .bind(Uuid::new_v4())
    .bind(s.company().as_uuid())
    .bind(channel)
    .bind(thread)
    .bind(message)
    .execute(s.pool())
    .await
    .unwrap();
    (message, Some(thread))
}

/// C3a, F9 sources 1 and 3 one at a time (AWC-P05-B1-F2): a run whose trigger message is a
/// customer's lists nothing, and so does a run whose input names a customer `source_message`;
/// each fixture leaves the other sources internal.
#[tokio::test]
async fn channel_call_authoring_discovery_external_sources() {
    let s = CallSetup::new().await;
    // Source 1: the run's (root's) trigger message. The message sits on a thread the run is not
    // on, so only the intent binding links it.
    let triggered = s.claim("discover-trigger").await;
    let front = super::support::new_channel(&s.tool.fixture, "trigger-front").await;
    let (message, thread) = customer_message(&s, Some(front)).await;
    let intent = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO workflow_message_intents (company_id,id,message_id,channel_id,thread_id,role,mentioned,admissible, \
           origin,hop_count,trace_targets,delivery_policy,correlation_id,available_at) \
         VALUES ($1,$2,$3,$4,$5,'to',false,true,'external',0,'[]'::jsonb,'deliver',$6,clock_timestamp() + interval '1 day')",
    )
    .bind(s.company().as_uuid())
    .bind(intent)
    .bind(message)
    .bind(front)
    .bind(thread.unwrap())
    .bind(Uuid::new_v4())
    .execute(s.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO workflow_message_intent_bindings (company_id,intent_id,binding_id,binding_revision,outcome,run_id) \
         SELECT company_id,$2,binding_id,binding_revision,'admitted',id FROM workflow_runs WHERE company_id=$1 AND id=$3",
    )
    .bind(s.company().as_uuid())
    .bind(intent)
    .bind(triggered.fence.scope.run.as_uuid())
    .execute(s.pool())
    .await
    .unwrap();
    assert_eq!(listing(&s, &triggered).await, DeskListing::External);
    s.tool
        .cancel(triggered.fence.scope, "discover-trigger-done")
        .await;

    // Source 3: the run's input names a customer message that is on no thread of the run.
    let sourced = s.claim("discover-source").await;
    let (message, _) = customer_message(&s, None).await;
    sqlx::query(
        "UPDATE workflow_runs SET input = jsonb_set(input, '{source_message}', to_jsonb($2::text)) WHERE id=$1",
    )
    .bind(sourced.fence.scope.run.as_uuid())
    .bind(message.to_string())
    .execute(s.pool())
    .await
    .unwrap();
    assert_eq!(listing(&s, &sourced).await, DeskListing::External);
    s.tool
        .cancel(sourced.fence.scope, "discover-source-done")
        .await;

    // A sibling run with neither lists.
    let plain = s.claim("discover-plain").await;
    assert_eq!(listed_ids(&listing(&s, &plain).await), vec![s.desk]);
}
