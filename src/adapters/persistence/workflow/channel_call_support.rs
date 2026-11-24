//! agent-workflow-call channel-call fixtures shared by the p02 snapshot tests and the p03
//! launch and resume tests: a claimed `agent.run` caller granted `channel.call` (`ToolSetup`) and
//! a desk whose selected responder runs as another person.
use super::support::{member, new_channel};
use super::workflow_tool_support::*;
use super::*;
use crate::adapters::persistence::workflow::channel_call_target::{self, CallDispatch, CallStart};
use crate::application::workflow::activation::ActivationRequest;
use crate::application::workflow::agent::AgentToolObservation;
use crate::application::workflow::channel_call::ChannelCallDispatch;
use crate::application::workflow::channel_reply::*;
use crate::application::workflow::lease::ClaimedWorkflow;
use crate::domain::workflow::MessageTrigger;
use chrono::Utc;
use serde_json::{Value, json};

pub(super) const DESTINATION: u128 = 0x0c01;

/// A responder that accepts any message, optionally declaring an intake.
pub(super) fn destination(intake: Option<Value>) -> Value {
    let input = match intake {
        None => json!({"type":"object"}),
        Some(fields) => json!({"type":"object","properties":{"fields":fields}}),
    };
    json!({
        "format_version": 1,
        "workflow_id": Uuid::nil().to_string(),
        "input_schema": input,
        "parameter_schema": {"type":"object"},
        "output_schema": {"type":"object"},
        "resources": [],
        "entry": "answer",
        "steps": {"answer": {"type":"data.map","with":{
            "value":{"literal":{"done":true}},
            "output_schema":{"literal":{"type":"object"}}},
            "routes":{"success":"$end"}}},
        "limits": {"max_steps": 10, "max_context_bytes": 1048576}
    })
}

/// A responder whose single human decision is reviewed by the desk unit's reviewer role.
pub(super) fn role_destination() -> Value {
    let mut source: Value =
        serde_json::from_str(&registry::example("decision.human").unwrap().source).unwrap();
    source["input_schema"] = json!({"type":"object","properties":{"value":{"type":"integer"}}});
    source["steps"]["start"]["with"]["data_schema"] = json!({"literal":true});
    source["steps"]["start"]["with"]["feedback_required"] = json!({"literal":[]});
    source["steps"]["start"]["with"]["reviewers"] = json!({"literal":["reviewer"]});
    source["steps"]["start"]["with"]["deadline"] =
        json!({"literal":(Utc::now() + chrono::Duration::minutes(20)).to_rfc3339()});
    source
}

pub(super) fn literal(body: &str) -> ChannelReplyContract {
    ChannelReplyContract {
        format_version: 1,
        subject: ReplyText::Literal {
            literal: "Status".into(),
        },
        body: ReplyText::Literal {
            literal: body.into(),
        },
    }
}

pub(super) struct CallSetup {
    pub(super) tool: ToolSetup,
    pub(super) desk: Uuid,
    pub(super) responder: WorkflowBindingId,
    pub(super) run_as: Uuid,
    pub(super) version: VersionId,
}

impl CallSetup {
    pub(super) async fn new() -> Self {
        Box::pin(Self::with(destination(None))).await
    }

    pub(super) async fn with(definition: Value) -> Self {
        Box::pin(Self::with_tools(
            definition,
            &[crate::entities::agent_capability::AgentBuiltinTool::ChannelCall],
        ))
        .await
    }

    /// The same setup, its agent holding `builtins` (Plan08 p04: `goal.status` beside the call).
    pub(super) async fn with_tools(
        definition: Value,
        builtins: &[crate::entities::agent_capability::AgentBuiltinTool],
    ) -> Self {
        let tool = Box::pin(ToolSetup::with_builtins(echo_child(), builtins)).await;
        let desk = new_channel(&tool.fixture, "finance").await;
        // The desk sits in its own unit under the root; the caller's write unit is the root.
        let unit = tool
            .persistence()
            .test_unit(tool.fixture.target.company.as_uuid(), "finance-unit")
            .await;
        sqlx::query(
            "UPDATE channels SET enabled=true, access_mode='public', department_id=$2 WHERE id=$1",
        )
        .bind(desk)
        .bind(unit)
        .execute(tool.persistence().pool())
        .await
        .unwrap();
        let run_as = member(&tool.fixture, "admin").await;
        let version = tool
            .fixture
            .publish_with_builtins(WorkflowId::new(Uuid::from_u128(DESTINATION)), definition)
            .await;
        let mut setup = Self {
            tool,
            desk,
            responder: WorkflowBindingId::new(Uuid::nil()),
            run_as,
            version,
        };
        setup.responder = setup.responder_as(run_as).await;
        setup.select(setup.responder, None).await.unwrap();
        setup
    }

    pub(super) fn p(&self) -> &PostgresPersistence {
        self.tool.persistence()
    }

    pub(super) fn pool(&self) -> &sqlx::PgPool {
        self.p().pool()
    }

    pub(super) fn company(&self) -> CompanyId {
        self.tool.fixture.target.company
    }

    pub(super) fn owner(&self) -> WorkflowActor {
        self.tool.fixture.target.actor
    }

    /// An active message-triggered binding of the destination on the desk, run as `run_as`.
    pub(super) async fn responder_as(&self, run_as: Uuid) -> WorkflowBindingId {
        self.responder_on(self.desk, self.version, run_as, json!({}))
            .await
    }

    /// An active message-triggered binding of `version` on `desk` with `params`.
    pub(super) async fn responder_on(
        &self,
        desk: Uuid,
        version: VersionId,
        run_as: Uuid,
        params: Value,
    ) -> WorkflowBindingId {
        let p = self.p();
        let bindings = BindingService::new(Preflight, p.clone(), p.clone(), p.clone());
        let target = BindingTarget {
            company: self.company(),
            actor: self.owner(),
            binding: WorkflowBindingId::new(Uuid::new_v4()),
        };
        let state = bindings
            .configure(ConfigureBinding {
                escalation: None,
                target,
                expected: None,
                association: RelatedAssociation::Channel(RelatedChannelId::new(desk)),
                version,
                params,
                resources: std::collections::BTreeMap::new(),
                run_as: Some(WorkflowActor::authenticated(run_as).unwrap()),
                message_trigger: Some(MessageTrigger::Any),
            })
            .await
            .unwrap();
        bindings
            .activate(SetBindingActivity {
                target,
                expected: state.revision,
            })
            .await
            .unwrap();
        target.binding
    }

    pub(super) async fn select(
        &self,
        binding: WorkflowBindingId,
        expected: Option<ReplySettingsRevision>,
    ) -> AppResult<ChannelReplySettings> {
        self.select_on(self.desk, binding, expected, None).await
    }

    /// Select `binding` as `desk`'s responder, with `approval_policy` as its final review.
    pub(super) async fn select_on(
        &self,
        desk: Uuid,
        binding: WorkflowBindingId,
        expected: Option<ReplySettingsRevision>,
        approval_policy: Option<ReplyApprovalPolicy>,
    ) -> AppResult<ChannelReplySettings> {
        self.select_with(desk, binding, expected, approval_policy, literal("Done"))
            .await
    }

    /// As `select_on`, with `contract` as the responder's reply contract.
    pub(super) async fn select_with(
        &self,
        desk: Uuid,
        binding: WorkflowBindingId,
        expected: Option<ReplySettingsRevision>,
        approval_policy: Option<ReplyApprovalPolicy>,
        contract: ChannelReplyContract,
    ) -> AppResult<ChannelReplySettings> {
        let state = self
            .p()
            .binding(self.company(), binding)
            .await
            .unwrap()
            .unwrap();
        self.p()
            .configure_channel_reply(ConfigureChannelReply {
                company: self.company(),
                target: crate::entities::address::AddressTarget::Desk(desk),
                actor: self.owner(),
                expected,
                approval_policy,
                responder: Some(ConfigureResponder {
                    binding,
                    expected_binding_revision: state.revision,
                    contract,
                }),
            })
            .await
    }

    pub(super) async fn clear(&self, expected: ReplySettingsRevision) {
        self.p()
            .configure_channel_reply(ConfigureChannelReply {
                company: self.company(),
                target: crate::entities::address::AddressTarget::Desk(self.desk),
                actor: self.owner(),
                expected: Some(expected),
                approval_policy: None,
                responder: None,
            })
            .await
            .unwrap();
    }

    pub(super) async fn claim(&self, key: &str) -> ClaimedWorkflow {
        self.tool.claim(key).await
    }

    /// A caller admitted on `channel` rather than on the company, so its write unit is the
    /// channel's unit.
    pub(super) async fn claim_from(&self, channel: Uuid, key: &str) -> ClaimedWorkflow {
        let scope = admitted_as(
            &self.tool.fixture,
            self.tool.target,
            RelatedAssociation::Channel(RelatedChannelId::new(channel)),
            key,
        )
        .await;
        self.p()
            .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
            .await
            .unwrap()
            .unwrap()
    }

    /// A caller admitted on a thread of its own front desk (doc 04 C.2's "thread the parent run
    /// is answering"). Returns the claim and the caller's thread.
    pub(super) async fn claim_in_thread(&self, key: &str) -> (ClaimedWorkflow, Uuid) {
        let front = new_channel(&self.tool.fixture, &format!("front-{key}")).await;
        let thread = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO threads (id, company_id, channel_id, subject) VALUES ($1,$2,$3,'Customer asks')",
        )
        .bind(thread)
        .bind(self.company().as_uuid())
        .bind(front)
        .execute(self.pool())
        .await
        .unwrap();
        let scope = admitted_as(
            &self.tool.fixture,
            self.tool.target,
            RelatedAssociation::Thread {
                target: crate::entities::address::AddressTarget::Desk(front),
                thread_id: RelatedThreadId::new(thread),
            },
            key,
        )
        .await;
        let claim = self
            .p()
            .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
            .await
            .unwrap()
            .unwrap();
        (claim, thread)
    }

    /// Another public desk in its own unit, answered by `version` run as `run_as` with `params`.
    pub(super) async fn add_desk(
        &self,
        name: &str,
        version: VersionId,
        params: Value,
    ) -> (Uuid, WorkflowBindingId) {
        let desk = new_channel(&self.tool.fixture, name).await;
        let unit = self
            .p()
            .test_unit(self.company().as_uuid(), &format!("{name}-unit"))
            .await;
        sqlx::query(
            "UPDATE channels SET enabled=true, access_mode='public', department_id=$2 WHERE id=$1",
        )
        .bind(desk)
        .bind(unit)
        .execute(self.pool())
        .await
        .unwrap();
        let binding = self.responder_on(desk, version, self.run_as, params).await;
        self.select_on(desk, binding, None, None).await.unwrap();
        (desk, binding)
    }

    /// The claim's saved `channel.call` through the production port, which commits.
    pub(super) async fn call(
        &self,
        claim: &ClaimedWorkflow,
        arguments: &Value,
    ) -> AppResult<AgentToolObservation> {
        let saved = self.tool.channel_call(arguments, claim).await;
        self.p().call_channel(claim.fence, saved).await
    }

    /// The call recorded for `caller`.
    pub(super) async fn record(&self, caller: RunId) -> CallRecord {
        sqlx::query_as(
            "SELECT call_id, child_run_id, thread_id, source_message_id FROM workflow_channel_calls \
             WHERE company_id=$1 AND run_id=$2",
        )
        .bind(self.company().as_uuid())
        .bind(caller.as_uuid())
        .fetch_one(self.pool())
        .await
        .unwrap()
    }

    /// Rows a call writes on its desk or as ingress, company-wide: threads and messages on
    /// `desk`, and intents, deliveries, reply contexts and external messages anywhere.
    pub(super) async fn desk_rows(&self, desk: Uuid) -> DeskRows {
        sqlx::query_as(
            "SELECT (SELECT count(*) FROM threads WHERE company_id=$1 AND channel_id=$2) AS threads, \
               (SELECT count(*) FROM thread_messages WHERE company_id=$1 AND channel_id=$2) AS messages, \
               (SELECT count(*) FROM workflow_message_intents WHERE company_id=$1) AS intents, \
               (SELECT count(*) FROM message_deliveries WHERE company_id=$1) AS deliveries, \
               (SELECT count(*) FROM channel_reply_contexts WHERE company_id=$1) AS reply_contexts, \
               (SELECT count(*) FROM external_messages WHERE company_id=$1) AS external",
        )
        .bind(self.company().as_uuid())
        .bind(desk)
        .fetch_one(self.pool())
        .await
        .unwrap()
    }

    pub(super) fn arguments(&self, body: &str) -> Value {
        json!({"channel_id": self.desk.to_string(), "content": {"body": body}})
    }

    /// The claim's saved `channel.call`, dispatched with `arguments` (which may differ from the
    /// saved ones to test a reused call ID), without committing.
    pub(super) async fn start_in(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        claim: &ClaimedWorkflow,
        arguments: &Value,
    ) -> AppResult<CallStart> {
        let saved = self.tool.channel_call(arguments, claim).await;
        Box::pin(channel_call_target::start_on(
            tx,
            &CallDispatch {
                fence: claim.fence,
                call: saved.call,
                arguments,
                origin: saved.origin,
            },
        ))
        .await
    }

    /// The dispatch, the caller's park when a child started, and the commit.
    pub(super) async fn start(
        &self,
        claim: &ClaimedWorkflow,
        arguments: &Value,
    ) -> AppResult<CallStart> {
        let mut tx = self.pool().begin().await?;
        let started = self.start_in(&mut tx, claim, arguments).await?;
        if let CallStart::Started { parked, .. } = &started {
            super::super::workflow_tool_calls::park_on(&mut tx, claim.fence, parked.wait.as_uuid())
                .await?;
        }
        tx.commit().await?;
        Ok(started)
    }

    pub(super) async fn records(&self) -> i64 {
        count(
            self.pool(),
            "SELECT count(*) FROM workflow_channel_calls WHERE company_id=$1",
            self.company(),
        )
        .await
    }

    /// The run's own write unit, as the cap check reads it: its write scope, else the root.
    pub(super) async fn write_unit(&self, run: RunId) -> Uuid {
        sqlx::query_scalar(
            "SELECT COALESCE((SELECT unit_id FROM workflow_run_scopes WHERE run_id=$1 \
               AND acting_principal_id IS NULL AND access='write'), \
             (SELECT id FROM departments WHERE company_id=$2 AND is_root))",
        )
        .bind(run.as_uuid())
        .bind(self.company().as_uuid())
        .fetch_one(self.pool())
        .await
        .unwrap()
    }

    /// Exhaust `unit`'s daily cap: a 1 µ cap with 5 µ already counted today.
    pub(super) async fn exhaust(&self, unit: Uuid) {
        crate::application::use_cases::company::CompanyPersistence::set_unit_budget(
            self.p(),
            self.owner().user_id(),
            self.company().as_uuid(),
            unit,
            "day",
            1,
        )
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO unit_spend (company_id, unit_id, period, period_start, spent_micros) \
             VALUES ($1, $2, 'day', date_trunc('day', clock_timestamp() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC', 5)",
        )
        .bind(self.company().as_uuid())
        .bind(unit)
        .execute(self.pool())
        .await
        .unwrap();
    }

    /// Grant or revoke the calling agent's `participate` right on the desk.
    pub(super) async fn participate(&self, granted: bool) {
        self.participate_on(self.desk, granted).await;
    }

    /// Grant or revoke the calling agent's `participate` capability on `desk`.
    pub(super) async fn participate_on(&self, desk: Uuid, granted: bool) {
        let principal: Uuid = sqlx::query_scalar(
            "SELECT id FROM principals WHERE company_id=$1 AND agent_id=$2 AND kind='agent'",
        )
        .bind(self.company().as_uuid())
        .bind(self.tool.agent)
        .fetch_one(self.pool())
        .await
        .unwrap();
        let statement = if granted {
            "INSERT INTO channel_principal_grants (company_id, channel_id, principal_id, capability, provenance) \
             VALUES ($1, $2, $3, 'participate', 'configured_allowlist')"
        } else {
            "DELETE FROM channel_principal_grants WHERE company_id=$1 AND channel_id=$2 AND principal_id=$3 \
             AND capability='participate'"
        };
        sqlx::query(statement)
            .bind(self.company().as_uuid())
            .bind(desk)
            .bind(principal)
            .execute(self.pool())
            .await
            .unwrap();
    }

    pub(super) async fn desk_unit(&self) -> Uuid {
        sqlx::query_scalar("SELECT department_id FROM channels WHERE id=$1")
            .bind(self.desk)
            .fetch_one(self.pool())
            .await
            .unwrap()
    }
}

pub(super) async fn count(pool: &sqlx::PgPool, sql: &str, company: CompanyId) -> i64 {
    sqlx::query_scalar(sql)
        .bind(company.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

pub(super) fn started(start: CallStart) -> RunId {
    match start {
        CallStart::Started { child, .. } => child,
        other => panic!("expected a started call, got {other:?}"),
    }
}

/// The code of an answered call: a tool error's `error.code` or a closure's `code`.
pub(super) fn answered(start: CallStart) -> String {
    let CallStart::Answered(value) = start else {
        panic!("expected an answered call, got {start:?}");
    };
    value["error"]["code"]
        .as_str()
        .or_else(|| value["code"].as_str())
        .unwrap_or_else(|| panic!("no code in {value}"))
        .to_owned()
}

/// The constraint a statement or a deferred check refused with.
pub(super) fn refused_by(error: sqlx::Error) -> String {
    error
        .as_database_error()
        .and_then(|error| error.constraint().map(str::to_owned))
        .unwrap_or_else(|| panic!("expected a named constraint refusal: {error}"))
}

pub(super) async fn immediate(
    tx: &mut Transaction<'_, Postgres>,
    constraint: &str,
) -> Result<(), String> {
    sqlx::query(&format!("SET CONSTRAINTS {constraint} IMMEDIATE"))
        .execute(&mut **tx)
        .await
        .map(|_| ())
        .map_err(refused_by)
}

/// Run one forged statement under a savepoint and report the constraint that refused it, at the
/// statement or at the deferred `constraint` made immediate; the savepoint is rolled back.
pub(super) async fn forged(
    tx: &mut Transaction<'_, Postgres>,
    statement: &str,
    (first, second): (Uuid, Uuid),
    constraint: &str,
) -> Result<(), String> {
    sqlx::query("SAVEPOINT forged")
        .execute(&mut **tx)
        .await
        .unwrap();
    let refused = match sqlx::query(statement)
        .bind(first)
        .bind(second)
        .execute(&mut **tx)
        .await
    {
        Err(error) => Err(refused_by(error)),
        Ok(_) => immediate(tx, constraint).await,
    };
    sqlx::query("ROLLBACK TO SAVEPOINT forged")
        .execute(&mut **tx)
        .await
        .unwrap();
    refused
}

/// A channel of another company, created with that company.
pub(super) async fn foreign_channel(s: &CallSetup) -> Uuid {
    let owner = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,username,email,password_hash) VALUES($1,$2,$3,'test')")
        .bind(owner)
        .bind(owner.simple().to_string())
        .bind(format!("{}@example.test", owner.simple()))
        .execute(s.pool())
        .await
        .unwrap();
    let company = Uuid::new_v4();
    crate::adapters::persistence::test_support::insert_company_with_owner(
        s.pool(),
        company,
        "Elsewhere",
        &format!("elsewhere-{}", company.simple()),
        owner,
    )
    .await;
    crate::adapters::persistence::test_support::create_channel(
        s.p(),
        company,
        crate::application::ports::channel::ChannelWrite {
            name: "desk".into(),
            slug: "desk".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id
}

/// The leading columns of a forged copy of the caller's record, up to `channel_id`.
pub(super) const FORGE: &str = "INSERT INTO workflow_channel_calls SELECT company_id,run_id,execution_id,gen_random_uuid(),arguments_digest, \
    agent_manifest_id,agent_id,agent_revision_id,agent_binding_id,";

pub(super) async fn assert_no_call_rows(s: &CallSetup) {
    assert_eq!(s.records().await, 0);
    assert_eq!(
        count(
            s.pool(),
            "SELECT count(*) FROM workflow_runs WHERE company_id=$1 AND parent_run_id IS NOT NULL",
            s.company()
        )
        .await,
        0
    );
}

/// The call child's run row, as V0 asserts it.
#[derive(Debug, PartialEq, sqlx::FromRow)]
pub(super) struct ChildRow {
    pub(super) actor_id: Uuid,
    pub(super) channel_id: Option<Uuid>,
    pub(super) thread_id: Option<Uuid>,
    pub(super) binding_id: Uuid,
    pub(super) ceiling: i16,
    pub(super) content: i16,
    pub(super) parent_run_id: Option<Uuid>,
    pub(super) delivery_policy: String,
}

/// A recorded call's keys.
#[derive(Debug, Clone, Copy, sqlx::FromRow)]
pub(super) struct CallRecord {
    pub(super) call_id: Uuid,
    pub(super) child_run_id: Uuid,
    pub(super) thread_id: Uuid,
    pub(super) source_message_id: Uuid,
}

/// What `CallSetup::desk_rows` counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::FromRow)]
pub(super) struct DeskRows {
    pub(super) threads: i64,
    pub(super) messages: i64,
    pub(super) intents: i64,
    pub(super) deliveries: i64,
    pub(super) reply_contexts: i64,
    pub(super) external: i64,
}

impl DeskRows {
    /// No row at all.
    pub(super) const NONE: Self = Self {
        threads: 0,
        messages: 0,
        intents: 0,
        deliveries: 0,
        reply_contexts: 0,
        external: 0,
    };
}

// Shared by the resume and response tests: one call driven to its result.

pub(super) fn result(observation: AgentToolObservation) -> Value {
    match observation {
        AgentToolObservation::Completed(value) => value,
        other => panic!("expected a tool result, got {other:?}"),
    }
}

pub(super) fn code(value: &Value) -> String {
    value["error"]["code"]
        .as_str()
        .or_else(|| value["code"].as_str())
        .unwrap_or_else(|| panic!("no code in {value}"))
        .to_owned()
}

/// The released answer of the default destination (`literal("Done")`).
pub(super) fn done(child: RunId) -> Value {
    json!({"closure": "done", "child_run_id": child.as_uuid(),
        "content": {"subject": "Status", "body": "Done"}})
}

/// The caller's continued claim, once its child has finished and the wakeup ran.
pub(super) async fn continued(s: &CallSetup, scope: ActivationRequest) -> ClaimedWorkflow {
    assert_eq!(
        s.p().resume_wait(scope).await.unwrap(),
        WaitProgress::Continued
    );
    s.p()
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .expect("the child's terminal fact continued the caller")
}

/// A call parked on a fresh claim, with its child run to its end.
pub(super) async fn finished(
    s: &CallSetup,
    key: &str,
    arguments: &Value,
) -> (ActivationRequest, RunId) {
    let claim = s.claim(key).await;
    let scope = claim.fence.scope;
    assert!(matches!(
        s.call(&claim, arguments).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    let child = s.tool.finish_child(scope).await;
    (scope, child.run)
}

/// Raise a finished child's facts so its absorption is observable: the call child's ceiling
/// is PUBLIC until request rules exist (decision U4). Triggers are bypassed for this one raw
/// setup update.
pub(super) async fn taint(s: &CallSetup, child: RunId) {
    let mut tx = s.pool().begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE workflow_runs SET knowledge_ceiling=2, content_level=2, content_exact=false WHERE id=$1",
    )
    .bind(child.as_uuid())
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

pub(super) async fn levels(s: &CallSetup, run: RunId) -> (i16, bool) {
    sqlx::query_as("SELECT content_level::smallint, content_exact FROM workflow_runs WHERE id=$1")
        .bind(run.as_uuid())
        .fetch_one(s.pool())
        .await
        .unwrap()
}
