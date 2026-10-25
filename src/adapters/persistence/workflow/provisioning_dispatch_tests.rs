//! p08 D2.2a/D2.4 against PostgreSQL: the provisioning tool's local action through the production
//! authority and effect, on a claimed job, and the "Create assistant channel" command; with
//! competing claimants, the requester rule, committed refusals, and transient faults (F043,
//! F046).
use super::*;
use crate::adapters::persistence::channel::{insert_channel_row_on, insert_channel_slugs};
use crate::adapters::persistence::workflow::action_dispatch::PostgresActionDispatch;
use crate::adapters::persistence::workflow::provisioning::{
    AssistantChannelService, ProvisioningActionAuthority, ProvisioningDeployment,
    ProvisioningEffects,
};
use crate::adapters::workflow_source::WorkflowSourceDecoder;
use crate::application::use_cases::agent::{AgentPersistence, AgentWrite};
use crate::application::use_cases::channel::ChannelWrite;
use crate::application::use_cases::company::{CompanyModelConnectionWrite, CompanyPersistence};
use crate::application::workflow::actions::*;
use crate::application::workflow::provisioning::*;

type Dispatch = PostgresActionDispatch<ProvisioningActionAuthority, ProvisioningEffects>;

fn templates() -> ProvisioningDeployment {
    ProvisioningDeployment::new(
        Arc::new(TemplateCatalogue::shipped(&WorkflowSourceDecoder).unwrap()),
        &[],
        "agents.test",
    )
}

fn dispatch(f: &AdmissionFixture) -> ActionService<Dispatch> {
    ActionService::new(PostgresActionDispatch::new(
        f.persistence().clone(),
        ProvisioningActionAuthority,
        ProvisioningEffects::new(templates()),
    ))
}

fn owner(f: &AdmissionFixture) -> WorkflowActor {
    f.binding.target.actor
}

fn company(f: &AdmissionFixture) -> CompanyId {
    f.binding.target.company
}

/// A company that can run `openai/gpt-4o-mini` (with a key).
async fn enable_model(f: &AdmissionFixture) {
    CompanyPersistence::replace_model_connections_for_user(
        f.persistence(),
        owner(f).user_id(),
        company(f).as_uuid(),
        vec![CompanyModelConnectionWrite {
            provider: "openai".into(),
            api_key: Some("test-key".into()),
            models: vec!["gpt-4o-mini".into()],
            is_default: true,
        }],
    )
    .await
    .unwrap();
}

/// A claimed job of a manual root (or of its child), in a company with a usable model.
async fn claimed(child: bool) -> (AdmissionFixture, ClaimedWorkflow) {
    let (f, scope) = if child {
        child_fixture().await
    } else {
        fixture().await
    };
    let claim = f
        .persistence()
        .settled()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    // The run's frozen bundle carries the trusted built-ins, as a Builder's publication would.
    let bundle: Vec<u8> = sqlx::query_scalar("SELECT bundle FROM workflow_runs WHERE id = $1")
        .bind(claim.fence.scope.run.as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    let bundle = publication::restore_bundle(&bundle, &WorkflowSourceDecoder).unwrap();
    let frozen = publication::freeze(
        crate::adapters::workflow_source::decode(bundle.compiled().source()).unwrap(),
        company(&f),
        bundle.compiled().graph().definition().version_id,
        DependencySnapshots {
            tools: [
                ProvisioningKind::ListAgents,
                ProvisioningKind::CreateAgentChannel,
            ]
            .into_iter()
            .map(|kind| builtin_provisioning_snapshot(company(&f), kind).unwrap())
            .collect(),
            ..Default::default()
        },
        vec![],
    )
    .unwrap();
    sqlx::query("UPDATE workflow_runs SET bundle = $2 WHERE id = $1")
        .bind(claim.fence.scope.run.as_uuid())
        .bind(store_bundle(&frozen).unwrap())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    enable_model(&f).await;
    (f, claim)
}

/// One saved model call of `kind` on the claimed execution, frozen and prepared.
async fn call(
    f: &AdmissionFixture,
    claim: &ClaimedWorkflow,
    kind: ProvisioningKind,
    arguments: Value,
) -> ActionDispatchRequest {
    let scope = ActionScope {
        company: claim.fence.scope.company,
        run: claim.fence.scope.run,
        execution: claim.fence.scope.execution,
    };
    let contract = builtin_provisioning_snapshot(scope.company, kind).unwrap();
    let request = ActionRequest {
        scope,
        operation_key: contract.contract.name.clone(),
        target: ActionTarget::Local {
            resource: scope.company.as_uuid(),
            resource_kind: TypeName::parse(PROVISIONING).unwrap(),
        },
        arguments,
        context: ActionPolicyContext {
            actor: owner(f).user_id(),
            capability_ceiling: vec![contract.policy.capability.clone()],
            approval_required: false,
        },
        contract,
    };
    let intent = ActionService::new(f.persistence().clone())
        .prepare_tool(
            request,
            ModelToolCallId::parse(Uuid::new_v4().to_string()).unwrap(),
        )
        .await
        .unwrap();
    ActionDispatchRequest {
        subject: intent.approval_subject(),
        fence: claim.fence,
        lease: policy(),
    }
}

fn spec(slug: &str) -> Value {
    json!({
        "name": "Helper", "slug": slug, "system_prompt": "Help the team.",
        "provider": "openai", "model": "gpt-4o-mini", "reply_mode": "autonomous"
    })
}

async fn committed(service: &ActionService<Dispatch>, request: &ActionDispatchRequest) -> Value {
    match service.dispatch_local(request).await.unwrap() {
        LocalDispatchResult::Committed(receipt) => receipt.result,
        _ => panic!("the call commits a receipt"),
    }
}

/// What provisioning wrote in the company: markers, receipts, agents, channel addresses,
/// provisioned workflows and channel bindings.
async fn written(f: &AdmissionFixture) -> Value {
    sqlx::query_scalar(
        "SELECT jsonb_build_object( \
           'markers', (SELECT count(*) FROM workflow_action_dispatches WHERE company_id = $1), \
           'receipts', (SELECT count(*) FROM workflow_action_receipts WHERE company_id = $1), \
           'agents', (SELECT count(*) FROM agents WHERE company_id = $1), \
           'channels', (SELECT count(*) FROM channel_slugs WHERE company_id = $1), \
           'workflows', (SELECT count(*) FROM workflow_definitions \
             WHERE company_id = $1 AND template_origin IS NOT NULL), \
           'bindings', (SELECT count(*) FROM workflow_bindings \
             WHERE company_id = $1 AND channel_id IS NOT NULL))",
    )
    .bind(company(f).as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap()
}

/// How much each count grew from `before` to `after`.
fn grew(before: &Value, after: &Value) -> Value {
    Value::Object(
        before
            .as_object()
            .unwrap()
            .iter()
            .map(|(kind, count)| {
                (
                    kind.clone(),
                    json!(after[kind].as_i64().unwrap() - count.as_i64().unwrap()),
                )
            })
            .collect(),
    )
}

/// A company member with `role` and a person principal.
async fn person(f: &AdmissionFixture, role: &str) -> WorkflowActor {
    let user = Uuid::new_v4();
    let pool = f.persistence().pool();
    sqlx::query("INSERT INTO users(id,username,email,password_hash) VALUES($1,$2,$3,'test')")
        .bind(user)
        .bind(user.simple().to_string())
        .bind(format!("{}@example.test", user.simple()))
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO company_members(id,company_id,user_id,role) VALUES($1,$2,$3,$4)")
        .bind(Uuid::new_v4())
        .bind(company(f).as_uuid())
        .bind(user)
        .bind(role)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO principals(id,company_id,kind,user_id,display_label) VALUES($1,$2,'person',$3,'member')")
        .bind(Uuid::new_v4()).bind(company(f).as_uuid()).bind(user).execute(pool).await.unwrap();
    WorkflowActor::authenticated(user).unwrap()
}

const ONE_ASSISTANT: &str =
    r#"{"markers": 1, "receipts": 1, "agents": 1, "channels": 1, "workflows": 1, "bindings": 1}"#;

/// F043: two holders of the same current fence dispatch one call at once; one assistant is
/// provisioned, and both (and a later restart) observe its one receipt.
#[tokio::test]
async fn provisioning_competing_claimants_create_one_assistant() {
    let (f, claim) = claimed(false).await;
    let request = call(
        &f,
        &claim,
        ProvisioningKind::CreateAgentChannel,
        spec("helper"),
    )
    .await;
    let before = written(&f).await;
    let (left, right) = (dispatch(&f), dispatch(&f));
    let barrier = Barrier::new(2);
    let (a, b) = tokio::join!(
        async {
            barrier.wait().await;
            committed(&left, &request).await
        },
        async {
            barrier.wait().await;
            committed(&right, &request).await
        }
    );
    assert_eq!(a, b);
    assert_eq!(a["agent"]["slug"], "helper");
    assert_eq!(a["reply_mode"], "autonomous");
    let company_slug: String = sqlx::query_scalar("SELECT slug::text FROM companies WHERE id = $1")
        .bind(company(&f).as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(
        a["channel"]["address"],
        format!("helper@{company_slug}.agents.test"),
        "F051: the channel's real address"
    );
    let expected: Value = serde_json::from_str(ONE_ASSISTANT).unwrap();
    assert_eq!(grew(&before, &written(&f).await), expected);
    assert_eq!(committed(&dispatch(&f), &request).await, a);
    assert_eq!(grew(&before, &written(&f).await), expected);

    // Everything is configured by the requester (the manual root's actor) and runs as them.
    let channel = Uuid::parse_str(a["channel"]["id"].as_str().unwrap()).unwrap();
    let binding = Uuid::parse_str(a["binding_id"].as_str().unwrap()).unwrap();
    let (access, created_by): (String, Value) =
        sqlx::query_as("SELECT access_mode, created_by FROM channels WHERE id = $1")
            .bind(channel)
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    assert_eq!(access, "team");
    assert_eq!(created_by["actor_id"], json!(owner(&f).user_id()));
    let (active, run_as, configured_by): (bool, Uuid, Uuid) = sqlx::query_as(
        "SELECT binding.active, revision.run_as_user_id, revision.actor_id \
         FROM workflow_bindings AS binding JOIN workflow_binding_revisions AS revision \
           ON revision.company_id = binding.company_id AND revision.binding_id = binding.id \
         WHERE binding.id = $1",
    )
    .bind(binding)
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert!(active);
    assert_eq!(
        (run_as, configured_by),
        (owner(&f).user_id(), owner(&f).user_id())
    );
    let events: Vec<(String, Uuid)> = sqlx::query_as(
        "SELECT event_kind, actor_id FROM workflow_binding_events WHERE binding_id = $1 \
         ORDER BY state_revision",
    )
    .bind(binding)
    .fetch_all(f.persistence().pool())
    .await
    .unwrap();
    assert_eq!(
        events,
        [
            ("configured".into(), owner(&f).user_id()),
            ("activated".into(), owner(&f).user_id())
        ]
    );
    let (provider, model, tools): (Option<String>, Option<String>, Vec<String>) =
        sqlx::query_as("SELECT provider, model, workflow_tools FROM agents WHERE slug = 'helper' AND company_id = $1")
            .bind(company(&f).as_uuid())
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    assert_eq!(
        (provider.as_deref(), model.as_deref(), tools.len()),
        (Some("openai"), Some("gpt-4o-mini"), 0)
    );
}

/// A stale holder (another worker, generation or attempt) writes nothing; the current one
/// provisions.
#[tokio::test]
async fn provisioning_stale_fences_write_nothing() {
    let (f, claim) = claimed(false).await;
    let request = call(
        &f,
        &claim,
        ProvisioningKind::CreateAgentChannel,
        spec("helper"),
    )
    .await;
    let before = written(&f).await;
    for mode in 0..3 {
        let mut stale = request.clone();
        match mode {
            0 => stale.fence.worker = worker(),
            1 => stale.fence.generation = WorkflowGeneration(Uuid::new_v4()),
            _ => stale.fence.attempt = WorkflowAttempt(99),
        }
        assert!(dispatch(&f).dispatch_local(&stale).await.is_err());
        assert_eq!(written(&f).await, before);
    }
    assert_eq!(
        committed(&dispatch(&f), &request).await["agent"]["slug"],
        "helper"
    );
}

/// D2.2a: a refusal commits its typed error as the receipt and provisions nothing; replaying the
/// call answers the same refusal.
#[tokio::test]
async fn provisioning_refusals_commit_a_receipt_and_no_effect() {
    let (f, claim) = claimed(false).await;
    AgentPersistence::create(
        f.persistence(),
        company(&f).as_uuid(),
        AgentWrite {
            name: "Taken".into(),
            slug: "taken".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let mut unknown_model = spec("fresh");
    unknown_model["model"] = json!("gpt-5");
    let mut provisioning_tool = spec("fresh");
    provisioning_tool["tools"] = json!([CREATE_AGENT_CHANNEL]);
    let mut unknown_skill = spec("fresh");
    unknown_skill["skills"] = json!(["missing"]);
    let mut bad_address = spec("fresh");
    bad_address["channel_slug"] = json!("not an address!");
    let mut bad_agent = spec("fresh");
    bad_agent["slug"] = json!("-edge");
    for (arguments, code) in [
        (spec("taken"), PROVISIONING_CONFLICT),
        (unknown_model, PROVISIONING_INVALID),
        (provisioning_tool, PROVISIONING_INVALID),
        (unknown_skill, PROVISIONING_INVALID),
        (bad_address, PROVISIONING_INVALID),
        (bad_agent, PROVISIONING_INVALID),
    ] {
        let before = written(&f).await;
        let request = call(&f, &claim, ProvisioningKind::CreateAgentChannel, arguments).await;
        let refused = committed(&dispatch(&f), &request).await;
        assert_eq!(refused["error"]["code"], code, "{refused}");
        assert_eq!(committed(&dispatch(&f), &request).await, refused);
        assert_eq!(
            grew(&before, &written(&f).await),
            json!({"markers": 1, "receipts": 1, "agents": 0, "channels": 0, "workflows": 0,
                "bindings": 0}),
            "{code}"
        );
    }
}

/// F046: an error inside the effect that is not a refusal rolls the whole action back, with no
/// marker, receipt or effect; the same call then provisions once.
#[tokio::test]
async fn provisioning_transient_fault_leaves_no_marker_receipt_or_effect() {
    let (f, claim) = claimed(false).await;
    let mut faulty = spec("faulty");
    faulty["channel_name"] = json!("Faulty provisioning target");
    let request = call(&f, &claim, ProvisioningKind::CreateAgentChannel, faulty).await;
    let pool = f.persistence().pool();
    sqlx::raw_sql(
        "CREATE FUNCTION provisioning_fault() RETURNS trigger LANGUAGE plpgsql AS $$ \
           BEGIN RAISE EXCEPTION 'injected provisioning fault'; END $$; \
         CREATE TRIGGER provisioning_fault BEFORE INSERT ON channels FOR EACH ROW \
           WHEN (NEW.name = 'Faulty provisioning target') EXECUTE FUNCTION provisioning_fault();",
    )
    .execute(pool)
    .await
    .unwrap();
    let before = written(&f).await;
    let failed = dispatch(&f).dispatch_local(&request).await;
    sqlx::raw_sql(
        "DROP TRIGGER provisioning_fault ON channels; DROP FUNCTION provisioning_fault();",
    )
    .execute(pool)
    .await
    .unwrap();
    assert!(failed.is_err(), "a fault is not a refusal");
    assert_eq!(written(&f).await, before);
    assert_eq!(
        committed(&dispatch(&f), &request).await["channel"]["slug"],
        "faulty"
    );
}

/// F043/F046: a channel creation that does not take the company lock claims the address after
/// the pre-check; the provisioning insert waits on it, and its unique violation becomes the
/// conflict refusal with nothing provisioned.
#[tokio::test]
async fn provisioning_losing_an_address_race_is_a_conflict_refusal() {
    let (f, claim) = claimed(false).await;
    let request = call(
        &f,
        &claim,
        ProvisioningKind::CreateAgentChannel,
        spec("raced"),
    )
    .await;
    let before = written(&f).await;
    let pool = f.persistence().pool();
    let mut rival = pool.begin().await.unwrap();
    let rival_channel = Uuid::new_v4();
    let write = ChannelWrite {
        name: "Rival".into(),
        slug: "raced".into(),
        ..ChannelWrite::default()
    };
    insert_channel_row_on(&mut rival, company(&f).as_uuid(), rival_channel, &write)
        .await
        .unwrap();
    insert_channel_slugs(
        &mut rival,
        company(&f).as_uuid(),
        rival_channel,
        "raced",
        &[],
    )
    .await
    .unwrap();
    let service = dispatch(&f);
    let provisioning = tokio::spawn({
        let request = request.clone();
        async move { service.dispatch_local(&request).await }
    });
    // Wait until the provisioning insert is blocked on the rival's address.
    for _ in 0..100 {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity WHERE wait_event_type = 'Lock' \
             AND query LIKE '%INSERT INTO channel_slugs%' AND datname = current_database()",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        if waiting > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    rival.commit().await.unwrap();
    let LocalDispatchResult::Committed(receipt) = provisioning.await.unwrap().unwrap() else {
        panic!("the refusal is committed")
    };
    assert_eq!(receipt.result["error"]["code"], PROVISIONING_CONFLICT);
    // Only the rival's address is new.
    assert_eq!(
        grew(&before, &written(&f).await),
        json!({"markers": 1, "receipts": 1, "agents": 0, "channels": 1, "workflows": 0,
            "bindings": 0})
    );
}

/// F048: a child's requester is its root's, found through the stored lineage.
#[tokio::test]
async fn provisioning_in_a_child_run_acts_for_the_roots_requester() {
    let (f, claim) = claimed(true).await;
    let request = call(
        &f,
        &claim,
        ProvisioningKind::CreateAgentChannel,
        spec("helper"),
    )
    .await;
    let created = committed(&dispatch(&f), &request).await;
    let binding = Uuid::parse_str(created["binding_id"].as_str().unwrap()).unwrap();
    let run_as: Uuid = sqlx::query_scalar(
        "SELECT run_as_user_id FROM workflow_binding_revisions WHERE binding_id = $1",
    )
    .bind(binding)
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert_eq!(run_as, owner(&f).user_id());
}

/// `list_company_agents` lists the company's agents by slug and its keyed models.
#[tokio::test]
async fn provisioning_lists_agents_and_models() {
    let (f, claim) = claimed(false).await;
    AgentPersistence::create(
        f.persistence(),
        company(&f).as_uuid(),
        AgentWrite {
            name: "Support".into(),
            slug: "support".into(),
            provider: Some("openai".into()),
            model: Some("gpt-4o-mini".into()),
            description: Some("Answers customers.".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let request = call(&f, &claim, ProvisioningKind::ListAgents, json!({})).await;
    let listed = committed(&dispatch(&f), &request).await;
    assert_eq!(
        listed["agents"],
        json!([{"slug": "support", "name": "Support", "description": "Answers customers.",
            "provider": "openai", "model": "gpt-4o-mini"}])
    );
    assert_eq!(
        listed["models"],
        json!([{"provider": "openai", "model": "gpt-4o-mini"}])
    );
    assert_eq!(listed["truncated"], false);
    // P08-D3 §D3.4: the local provisioning marker exists but no reconciliation names it, so the
    // run detail lists no action.
    let scope = claim.fence.scope;
    let kinds: Vec<String> = sqlx::query_scalar(
        "SELECT effect_kind FROM workflow_action_dispatches WHERE company_id = $1 AND run_id = $2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_all(f.persistence().pool())
    .await
    .unwrap();
    assert_eq!(kinds, ["local"]);
    use crate::application::workflow::reads::WorkflowReads;
    let detail = f
        .persistence()
        .run_detail(scope.company, scope.run)
        .await
        .unwrap()
        .unwrap();
    assert!(detail.actions.is_empty(), "{:?}", detail.actions);
}

/// F050: a directory larger than a receipt may hold is listed within its bound, cut names and
/// descriptions, and says it was truncated; the call still commits.
#[tokio::test]
async fn provisioning_lists_an_oversized_directory_within_its_bound() {
    let (f, claim) = claimed(false).await;
    for index in 0..(MAX_LISTED_AGENTS + 5) {
        AgentPersistence::create(
            f.persistence(),
            company(&f).as_uuid(),
            AgentWrite {
                name: format!("Agent {index} {}", "n".repeat(200)),
                slug: format!("agent-{index:03}"),
                description: Some("d".repeat(600)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    let request = call(&f, &claim, ProvisioningKind::ListAgents, json!({})).await;
    let listed = committed(&dispatch(&f), &request).await;
    assert_eq!(listed["truncated"], true);
    let agents = listed["agents"].as_array().unwrap();
    assert!(
        !agents.is_empty() && agents.len() < MAX_LISTED_AGENTS,
        "{}",
        agents.len()
    );
    assert_eq!(agents[0]["slug"], "agent-000");
    for agent in agents {
        assert!(agent["name"].as_str().unwrap().chars().count() <= MAX_LISTED_NAME_CHARS);
        assert_eq!(
            agent["description"].as_str().unwrap().chars().count(),
            MAX_LISTED_DESCRIPTION_CHARS
        );
    }
    assert!(listed.to_string().len() <= MAX_LISTING_BYTES + 64);
    assert_eq!(listed["models"].as_array().unwrap().len(), 1);
}

fn command(key: &str, channel: &str) -> AssistantChannelRequest {
    AssistantChannelRequest {
        agent_slug: "support".into(),
        channel_slug: channel.into(),
        channel_name: "Support desk".into(),
        reply_mode: ReplyMode::Reviewed,
        command_key: key.into(),
    }
}

/// A company with the `support` agent on an explicit model, for the UI command.
async fn command_fixture() -> AdmissionFixture {
    let (f, _) = fixture().await;
    enable_model(&f).await;
    AgentPersistence::create(
        f.persistence(),
        company(&f).as_uuid(),
        AgentWrite {
            name: "Support".into(),
            slug: "support".into(),
            provider: Some("openai".into()),
            model: Some("gpt-4o-mini".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    f
}

fn service(f: &AdmissionFixture) -> AssistantChannelService {
    AssistantChannelService::new(f.persistence().clone(), templates())
}

/// D2.4, F041: the command record decides replay and conflict; a retry returns the stored result
/// and writes nothing.
#[tokio::test]
async fn assistant_channel_command_replays_by_key_and_refuses_a_changed_request() {
    let f = command_fixture().await;
    let before = written(&f).await;
    let created = service(&f)
        .create_assistant_channel(company(&f), owner(&f), command("desk-1", "desk"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(created["reply_mode"], "reviewed");
    let after = written(&f).await;
    assert_eq!(
        service(&f)
            .create_assistant_channel(company(&f), owner(&f), command("desk-1", "desk"))
            .await
            .unwrap()
            .unwrap(),
        created
    );
    assert_eq!(written(&f).await, after);
    assert!(matches!(
        service(&f)
            .create_assistant_channel(company(&f), owner(&f), command("desk-1", "other"))
            .await,
        Err(AppError::Conflict(_))
    ));
    // The reviewed binding's reviewer is the caller.
    let binding = Uuid::parse_str(created["binding_id"].as_str().unwrap()).unwrap();
    let params: Value =
        sqlx::query_scalar("SELECT params FROM workflow_binding_revisions WHERE binding_id = $1")
            .bind(binding)
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    assert_eq!(params, json!({"reviewer": owner(&f).user_id().to_string()}));
    // A member may not; an agent without an explicit model cannot answer a workflow.
    let member = person(&f, "member").await;
    assert!(matches!(
        service(&f)
            .create_assistant_channel(company(&f), member, command("desk-2", "desk-2"))
            .await,
        Err(AppError::NotFound(_))
    ));
    AgentPersistence::create(
        f.persistence(),
        company(&f).as_uuid(),
        AgentWrite {
            name: "Inherits".into(),
            slug: "inherits".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let mut inherits = command("desk-3", "desk-3");
    inherits.agent_slug = "inherits".into();
    let refused = service(&f)
        .create_assistant_channel(company(&f), owner(&f), inherits)
        .await
        .unwrap()
        .unwrap_err();
    assert!(
        matches!(refused, ProvisioningRefusal::Invalid(_)),
        "{refused:?}"
    );
    assert_eq!(
        grew(&after, &written(&f).await),
        json!({"markers": 0, "receipts": 0, "agents": 1, "channels": 0, "workflows": 0,
            "bindings": 0}),
        "only the inheriting agent created above"
    );
    assert_eq!(
        grew(&before, &after),
        json!({"markers": 0, "receipts": 0, "agents": 0, "channels": 1, "workflows": 1,
            "bindings": 1}),
        "one assistant channel for the existing agent"
    );
}

/// F043: commands racing on one key make one assistant and one replay; commands racing on one
/// address under different keys serialize on the company, and the second is the conflict.
#[tokio::test]
async fn assistant_channel_commands_race_to_one_assistant() {
    let f = command_fixture().await;
    let before = written(&f).await;
    let barrier = Barrier::new(2);
    let (a, b) = tokio::join!(
        async {
            barrier.wait().await;
            service(&f)
                .create_assistant_channel(company(&f), owner(&f), command("same", "same"))
                .await
        },
        async {
            barrier.wait().await;
            service(&f)
                .create_assistant_channel(company(&f), owner(&f), command("same", "same"))
                .await
        }
    );
    assert_eq!(a.unwrap().unwrap(), b.unwrap().unwrap());
    assert_eq!(grew(&before, &written(&f).await)["bindings"], 1);

    let (c, d) = tokio::join!(
        async {
            barrier.wait().await;
            service(&f)
                .create_assistant_channel(company(&f), owner(&f), command("one", "contested"))
                .await
        },
        async {
            barrier.wait().await;
            service(&f)
                .create_assistant_channel(company(&f), owner(&f), command("two", "contested"))
                .await
        }
    );
    let outcomes = [c.unwrap(), d.unwrap()];
    assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
    assert!(
        outcomes
            .iter()
            .any(|outcome| matches!(outcome, Err(ProvisioningRefusal::Conflict(_))))
    );
    assert_eq!(grew(&before, &written(&f).await)["bindings"], 2);
    let records: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_provisioning_commands WHERE company_id = $1",
    )
    .bind(company(&f).as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert_eq!(records, 2, "a refused command records nothing");
}

/// F047: an agent's grant of a provisioning name captures the trusted built-in, never an MCP
/// tool of that name another step captured; that capture makes the grant ambiguous instead.
#[tokio::test]
async fn provisioning_grants_capture_the_built_in_or_refuse_an_ambiguous_capture() {
    use crate::adapters::persistence::workflow::agent_publication::{
        agent_selection, capture_agents_on,
    };
    use crate::application::workflow::compiler::{rebase_workflow_id, retarget_agent};
    let f = command_fixture().await;
    AgentPersistence::create(
        f.persistence(),
        company(&f).as_uuid(),
        AgentWrite {
            name: "Builder".into(),
            slug: "builder".into(),
            provider: Some("openai".into()),
            model: Some("gpt-4o-mini".into()),
            workflow_tools: Some(vec![TypeName::parse(CREATE_AGENT_CHANNEL).unwrap()]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let catalogue = TemplateCatalogue::shipped(&WorkflowSourceDecoder).unwrap();
    let template = catalogue.template(AUTONOMOUS_RESPONSE).unwrap();
    let workflow = WorkflowId::new(Uuid::new_v4());
    let source = retarget_agent(
        &WorkflowSourceDecoder,
        template.source(),
        REPLY_AGENT_STEP,
        "builder",
    )
    .and_then(|source| rebase_workflow_id(&WorkflowSourceDecoder, &source, workflow))
    .unwrap();
    let draft = CompanyWorkflowDraft::from_template(company(&f), workflow, template, source);
    let selection = agent_selection(&draft).unwrap().unwrap();
    let capture = async |tools: Vec<ToolSnapshot>| {
        let mut captured = PublicationDependencies {
            snapshots: DependencySnapshots {
                tools,
                ..Default::default()
            },
            children: vec![],
        };
        let mut tx = f.persistence().pool().begin().await.unwrap();
        let result = capture_agents_on(&mut tx, company(&f), &selection, &mut captured).await;
        tx.rollback().await.unwrap();
        result.map(|()| captured.snapshots.tools)
    };

    let tools = capture(vec![]).await.unwrap();
    let granted: Vec<_> = tools
        .iter()
        .filter(|tool| tool.contract.name.as_str() == CREATE_AGENT_CHANNEL)
        .collect();
    assert_eq!(granted.len(), 1);
    assert!(is_trusted_provisioning_snapshot(granted[0]).unwrap());

    let mut mcp =
        builtin_provisioning_snapshot(company(&f), ProvisioningKind::CreateAgentChannel).unwrap();
    mcp.contract.connection = Some(ResourceName::parse("crm").unwrap());
    let Err(refused) = capture(vec![mcp]).await else {
        panic!("an MCP tool of the granted name makes the grant ambiguous")
    };
    assert!(
        matches!(&refused, AppError::BadRequest(message) if message.contains("conflicts")),
        "{refused:?}"
    );
}
