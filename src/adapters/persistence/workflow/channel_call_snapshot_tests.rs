//! agent-workflow-call p02 (gate AWC-P02-D1 r3) against PostgreSQL: one channel call admitted in
//! the dispatch transaction, its child written by the production child writer under the
//! destination's own identity and dependency scope, and the SQL guards that refuse forged rows.
//! The caller is a claimed `agent.run` step with a live fence (`ToolSetup`); the destination is a
//! desk whose selected responder runs as another person.
use super::channel_call_support::*;
use super::support::{member, new_channel};
use super::*;
use crate::adapters::persistence::workflow::channel_call_target::{
    self, CallStart, destination_refusal,
};
use crate::application::workflow::activation::ActivationRequest;
use crate::application::workflow::channel_call::{
    AUDIENCE_REFUSED, DESTINATION_UNAVAILABLE, INTAKE_INVALID, INTAKE_NOT_DECLARED, INVALID_INPUT,
    NO_RESPONDER, TARGET_UNAVAILABLE,
};
use crate::application::workflow::channel_reply::*;
use chrono::Utc;
use serde_json::json;

// ---------------------------------------------------------------------------------------------
// V0, V1 — the composed write, the record and replay.

#[tokio::test]
async fn channel_call_snapshot_positive_control_commits() {
    let s = CallSetup::new().await;
    let claim = s.claim("call-v0").await;
    let caller = claim.fence.scope.run;
    let child = started(
        s.start(&claim, &s.arguments("Is customer 42 over its limit?"))
            .await
            .unwrap(),
    );
    // The child runs the destination as configured, on its call's fresh desk thread, at the
    // request's level.
    let thread: Uuid =
        sqlx::query_scalar("SELECT thread_id FROM workflow_channel_calls WHERE child_run_id=$1")
            .bind(child.as_uuid())
            .fetch_one(s.pool())
            .await
            .unwrap();
    let row: ChildRow = sqlx::query_as(
        "SELECT actor_id, channel_id, thread_id, binding_id, knowledge_ceiling::smallint AS ceiling, \
         content_level::smallint AS content, parent_run_id, delivery_policy FROM workflow_runs WHERE id=$1",
    )
    .bind(child.as_uuid())
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(
        row,
        ChildRow {
            actor_id: s.run_as,
            channel_id: Some(s.desk),
            thread_id: Some(thread),
            binding_id: s.responder.as_uuid(),
            ceiling: 0,
            content: 0,
            parent_run_id: Some(caller.as_uuid()),
            delivery_policy: "deliver".into(),
        }
    );
    // Its own scope: the manifest's root is the child, inside the caller's budget tree.
    let (scope_root, budget_root, caller_root): (Uuid, Uuid, Uuid) = sqlx::query_as(
        "SELECT ownership.root_run_id, child.root_run_id, caller.root_run_id FROM run_dependency_manifests AS ownership \
         JOIN workflow_run_budgets AS child ON child.company_id=ownership.company_id AND child.run_id=ownership.run_id \
         JOIN workflow_run_budgets AS caller ON caller.company_id=child.company_id AND caller.run_id=child.parent_run_id \
         WHERE ownership.run_id=$1",
    )
    .bind(child.as_uuid())
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(scope_root, child.as_uuid());
    assert_eq!(budget_root, caller_root);
    // Its unit scopes are the desk's, not copies of the caller's (the root here).
    let desk_unit = s.desk_unit().await;
    assert_eq!(s.write_unit(child).await, desk_unit);
    assert_eq!(
        s.write_unit(caller).await,
        s.p().test_root_unit(s.company().as_uuid()).await
    );
    assert_ne!(s.write_unit(caller).await, desk_unit);
    // The record is the settings revision that was current.
    let (revision, recorded_binding, run_as, recorded_child): (i64, Uuid, Uuid, Uuid) = sqlx::query_as(
        "SELECT settings_revision, binding_id, run_as_user_id, child_run_id FROM workflow_channel_calls WHERE run_id=$1",
    )
    .bind(caller.as_uuid())
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(
        (revision, recorded_binding, run_as, recorded_child),
        (1, s.responder.as_uuid(), s.run_as, child.as_uuid())
    );
}

#[tokio::test]
async fn channel_call_snapshot_record_is_immutable_and_replays() {
    let s = CallSetup::new().await;
    let claim = s.claim("call-v1").await;
    let arguments = s.arguments("First request");
    let mut tx = s.pool().begin().await.unwrap();
    let child = started(s.start_in(&mut tx, &claim, &arguments).await.unwrap());
    // The same call answers from its record; settings are not read again.
    match s.start_in(&mut tx, &claim, &arguments).await.unwrap() {
        CallStart::Replayed { child: replayed } => assert_eq!(replayed, child),
        other => panic!("expected a replay, got {other:?}"),
    }
    // The same call ID with other arguments is refused and writes nothing.
    assert_eq!(
        answered(
            s.start_in(&mut tx, &claim, &s.arguments("Changed"))
                .await
                .unwrap()
        ),
        INVALID_INPUT
    );
    tx.commit().await.unwrap();
    assert_eq!(s.records().await, 1);
    for statement in [
        "UPDATE workflow_channel_calls SET arguments_digest=repeat('0',64) WHERE company_id=$1",
        "DELETE FROM workflow_channel_calls WHERE company_id=$1",
    ] {
        let error = sqlx::query(statement)
            .bind(s.company().as_uuid())
            .execute(s.pool())
            .await
            .unwrap_err();
        assert_eq!(refused_by(error), "workflow_channel_call_immutable");
    }
}

// ---------------------------------------------------------------------------------------------
// V2 — forged rows, each refused by its own constraint.

#[tokio::test]
async fn channel_call_snapshot_sql_refuses_forged_rows() {
    let s = CallSetup::new().await;
    let claim = s.claim("call-v2").await;
    let caller = claim.fence.scope;
    let mut tx = s.pool().begin().await.unwrap();
    let child = started(
        s.start_in(&mut tx, &claim, &s.arguments("Request"))
            .await
            .unwrap(),
    );
    let foreign = foreign_channel(&s).await;
    // Each case: savepoint, one forged fact, the one constraint that refuses it, rollback.
    let foreign_case = format!(
        "{FORGE} '{foreign}'::uuid,binding_id,binding_revision, \
           settings_revision,contract,approval_policy,run_as_user_id,run_id,agent_manifest_id,thread_id,gen_random_uuid(),created_at \
         FROM workflow_channel_calls WHERE company_id=$1 AND run_id=$2"
    );
    let cases: [(&str, &str, &str); 6] = [
        (
            "the same invocation key twice",
            "INSERT INTO workflow_channel_calls SELECT * FROM workflow_channel_calls WHERE company_id=$1 AND run_id=$2",
            "workflow_channel_calls_pkey",
        ),
        (
            "another company's channel",
            &foreign_case,
            "workflow_channel_call_binding_fk",
        ),
        (
            "a child that is not the caller's child (the caller itself)",
            "INSERT INTO workflow_channel_calls SELECT company_id,run_id,execution_id,gen_random_uuid(),arguments_digest, \
               agent_manifest_id,agent_id,agent_revision_id,agent_binding_id,channel_id,binding_id,binding_revision, \
               settings_revision,contract,approval_policy,run_as_user_id,run_id,agent_manifest_id,thread_id,gen_random_uuid(),created_at \
             FROM workflow_channel_calls WHERE company_id=$1 AND run_id=$2",
            "workflow_channel_call_snapshot",
        ),
        (
            "a binding outside the company's desk",
            "INSERT INTO workflow_channel_calls SELECT company_id,run_id,execution_id,gen_random_uuid(),arguments_digest, \
               agent_manifest_id,agent_id,agent_revision_id,agent_binding_id,channel_id,gen_random_uuid(),binding_revision, \
               settings_revision,contract,approval_policy,run_as_user_id,run_id,agent_manifest_id,thread_id,gen_random_uuid(),created_at \
             FROM workflow_channel_calls WHERE company_id=$1 AND run_id=$2",
            "workflow_channel_call_binding_fk",
        ),
        (
            "a scope manifest that is not the named child's",
            "INSERT INTO workflow_channel_calls SELECT company_id,run_id,execution_id,gen_random_uuid(),arguments_digest, \
               agent_manifest_id,agent_id,agent_revision_id,agent_binding_id,channel_id,binding_id,binding_revision, \
               settings_revision,contract,approval_policy,run_as_user_id,run_id,scope_manifest_id,thread_id,gen_random_uuid(),created_at \
             FROM workflow_channel_calls WHERE company_id=$1 AND run_id=$2",
            "workflow_channel_call_scope_fk",
        ),
        (
            "settings changed after the capture",
            "UPDATE channel_reply_settings SET revision=revision+1 WHERE company_id=$1 AND channel_id=(SELECT channel_id FROM workflow_channel_calls WHERE company_id=$1 AND run_id=$2)",
            "workflow_channel_call_snapshot",
        ),
    ];
    for (case, statement, constraint) in cases {
        let refused = forged(
            &mut tx,
            statement,
            (s.company().as_uuid(), caller.run.as_uuid()),
            constraint,
        )
        .await;
        assert_eq!(refused, Err(constraint.to_owned()), "{case}");
    }
    // The child's own identity: an actor other than `run_as` is refused.
    let refused = forged(
        &mut tx,
        "UPDATE workflow_runs SET actor_id=$2 WHERE id=$1",
        (child.as_uuid(), s.owner().user_id()),
        "workflow_channel_call_snapshot",
    )
    .await;
    assert_eq!(
        refused,
        Err("workflow_channel_call_snapshot".to_owned()),
        "child actor is not run_as"
    );
    // Untampered, every new guard holds.
    for constraint in ["workflow_channel_call_snapshot", "run_dependency_scope"] {
        assert_eq!(immediate(&mut tx, constraint).await, Ok(()), "{constraint}");
    }
    tx.rollback().await.unwrap();
}

// ---------------------------------------------------------------------------------------------
// V3, V4, V5 — competing writers.

#[tokio::test]
async fn channel_call_snapshot_duplicate_key_converges() {
    let s = CallSetup::new().await;
    let claim = s.claim("call-v3").await;
    let arguments = s.arguments("Once");
    // Saved once before the race: both dispatch the same saved call.
    s.tool.channel_call(&arguments, &claim).await;
    // Two transactions dispatch the same saved call; exactly one is admitted.
    let (a, b) = tokio::join!(s.start(&claim, &arguments), s.start(&claim, &arguments));
    assert_eq!(
        usize::from(a.is_ok()) + usize::from(b.is_ok()),
        1,
        "{a:?} {b:?}"
    );
    // The fence is the first arbiter: the loser finds the execution no longer its own. The
    // primary key is the backstop, refused directly in V2's "same invocation key twice".
    assert!(
        matches!(
            if a.is_ok() { &b } else { &a },
            Err(AppError::Execution(
                crate::app_error::ExecutionFailure::OwnershipLost
            ))
        ),
        "{a:?} {b:?}"
    );
    assert_eq!(s.records().await, 1);
}

#[tokio::test]
async fn channel_call_snapshot_reconfiguration_selects_one_whole_revision() {
    let s = CallSetup::new().await;
    let other_run_as = member(&s.tool.fixture, "admin").await;
    let other = s.responder_as(other_run_as).await;
    let claim = s.claim("call-v4").await;
    let arguments = s.arguments("During reconfiguration");
    let (call, replaced) = tokio::join!(
        s.start(&claim, &arguments),
        s.select(other, Some(ReplySettingsRevision::try_from(1).unwrap()))
    );
    replaced.unwrap();
    match call {
        Ok(CallStart::Started { child, .. }) => {
            let (revision, binding, run_as, actor): (i64, Uuid, Uuid, Uuid) = sqlx::query_as(
                "SELECT call.settings_revision, call.binding_id, call.run_as_user_id, run.actor_id \
                 FROM workflow_channel_calls AS call JOIN workflow_runs AS run ON run.id=call.child_run_id \
                 WHERE call.child_run_id=$1",
            )
            .bind(child.as_uuid())
            .fetch_one(s.pool())
            .await
            .unwrap();
            assert!(
                [
                    (1, s.responder.as_uuid(), s.run_as, s.run_as),
                    (2, other.as_uuid(), other_run_as, other_run_as)
                ]
                .contains(&(revision, binding, run_as, actor)),
                "a mixture: {revision} {binding} {run_as} {actor}"
            );
        }
        Ok(CallStart::Stale) => assert_no_call_rows(&s).await,
        other => panic!("unexpected {other:?}"),
    }
    assert!(s.records().await <= 1);
}

#[tokio::test]
async fn channel_call_snapshot_competes_with_run_as_account_write() {
    let s = CallSetup::new().await;
    let claim = s.claim("call-v5").await;
    let arguments = s.arguments("While the account changes");
    let account = async {
        let mut tx = s.pool().begin().await?;
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
            .bind(s.run_as)
            .execute(&mut *tx)
            .await?;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        sqlx::query("SELECT 1 FROM companies WHERE id=$1 FOR NO KEY UPDATE")
            .bind(s.company().as_uuid())
            .execute(&mut *tx)
            .await?;
        tx.commit().await
    };
    let (call, account) = tokio::join!(s.start(&claim, &arguments), account);
    account.unwrap();
    started(call.unwrap());
}

// ---------------------------------------------------------------------------------------------
// V6, V11, V12, V13, V14, V15 — refusals before or under the child's savepoint.

#[tokio::test]
async fn channel_call_snapshot_revoked_or_unready_target_is_denied() {
    let s = CallSetup::new().await;
    // `run_as` loses desk management after the responder was selected.
    sqlx::query(
        "DELETE FROM department_roles WHERE company_id=$1 AND role='admin' \
         AND principal_id=(SELECT id FROM principals WHERE company_id=$1 AND user_id=$2)",
    )
    .bind(s.company().as_uuid())
    .bind(s.run_as)
    .execute(s.pool())
    .await
    .unwrap();
    let claim = s.claim("call-v6").await;
    assert_eq!(
        answered(s.start(&claim, &s.arguments("Denied")).await.unwrap()),
        DESTINATION_UNAVAILABLE
    );
    assert_no_call_rows(&s).await;
}

#[tokio::test]
async fn channel_call_target_facts_decide_before_any_row() {
    let s = CallSetup::new().await;
    // A claim holds one saved call, so each case dispatches from its own; all are claimed
    // before the first dispatch runs the agent. An answered call keeps its job running, and a
    // company runs two jobs at once by default, so the desk states are the next test.
    let claims = [s.claim("call-v11-1").await, s.claim("call-v11-2").await];
    // A members-only desk without the agent's grant is one error, naming nothing else.
    sqlx::query("UPDATE channels SET access_mode='members' WHERE id=$1")
        .bind(s.desk)
        .execute(s.pool())
        .await
        .unwrap();
    assert_eq!(
        answered(s.start(&claims[0], &s.arguments("Members")).await.unwrap()),
        TARGET_UNAVAILABLE
    );
    // A foreign or missing desk is the same error.
    let missing = json!({"channel_id": Uuid::new_v4().to_string(), "content": {"body": "x"}});
    assert_eq!(
        answered(s.start(&claims[1], &missing).await.unwrap()),
        TARGET_UNAVAILABLE
    );
    assert_no_call_rows(&s).await;
}

#[tokio::test]
async fn channel_call_target_facts_desk_states_decide_before_any_row() {
    let s = CallSetup::new().await;
    let claims = [s.claim("call-v11-3").await, s.claim("call-v11-4").await];
    // A disabled desk declines.
    sqlx::query("UPDATE channels SET enabled=false WHERE id=$1")
        .bind(s.desk)
        .execute(s.pool())
        .await
        .unwrap();
    assert_eq!(
        answered(s.start(&claims[0], &s.arguments("Disabled")).await.unwrap()),
        crate::application::workflow::channel_call::DESK_DISABLED
    );
    sqlx::query("UPDATE channels SET enabled=true WHERE id=$1")
        .bind(s.desk)
        .execute(s.pool())
        .await
        .unwrap();
    // A desk without a responder declines.
    s.clear(ReplySettingsRevision::try_from(1).unwrap()).await;
    assert_eq!(
        answered(s.start(&claims[1], &s.arguments("Nobody")).await.unwrap()),
        NO_RESPONDER
    );
    assert_no_call_rows(&s).await;
}

#[tokio::test]
async fn channel_call_target_facts_participate_grant_opens_members_only_desk() {
    let s = CallSetup::new().await;
    sqlx::query("UPDATE channels SET access_mode='members' WHERE id=$1")
        .bind(s.desk)
        .execute(s.pool())
        .await
        .unwrap();
    // The agent's grant is its write right on a members-only desk (U2).
    s.participate(true).await;
    let granted = s.claim("call-grant").await;
    started(s.start(&granted, &s.arguments("With grant")).await.unwrap());
    // Revoked, the same desk is unavailable and nothing more is written.
    s.participate(false).await;
    let revoked = s.claim("call-revoked").await;
    assert_eq!(
        answered(s.start(&revoked, &s.arguments("Revoked")).await.unwrap()),
        TARGET_UNAVAILABLE
    );
    assert_eq!(s.records().await, 1);
}

#[tokio::test]
async fn channel_call_target_facts_archived_unit_declines() {
    let s = CallSetup::new().await;
    let claim = s.claim("call-archived").await;
    crate::adapters::persistence::test_support::department_use_cases(s.p())
        .archive_department(
            s.owner().user_id(),
            s.company().as_uuid(),
            crate::entities::department::DepartmentId::new(s.desk_unit().await),
            crate::application::ports::department::OpenWork::Refuse,
        )
        .await
        .unwrap();
    assert_eq!(
        answered(s.start(&claim, &s.arguments("Archived")).await.unwrap()),
        AppError::UNIT_ARCHIVED
    );
    assert_no_call_rows(&s).await;
}

#[tokio::test]
async fn channel_call_target_facts_ancestry_includes_self_and_nested_calls() {
    let s = CallSetup::new().await;
    let claim = s.claim("call-ancestry").await;
    let child = started(s.start(&claim, &s.arguments("Outer")).await.unwrap());
    let other = new_channel(&s.tool.fixture, "elsewhere").await;
    let mut db = s.pool().acquire().await.unwrap();
    let scope = |run: RunId| ActivationRequest {
        run,
        ..claim.fence.scope
    };
    // From the call child, its own desk is in its ancestry (a self-call); another desk is not.
    assert!(
        channel_call_target::in_ancestry_on(&mut db, scope(child), s.desk)
            .await
            .unwrap()
    );
    assert!(
        !channel_call_target::in_ancestry_on(&mut db, scope(child), other)
            .await
            .unwrap()
    );
    // From the caller, which runs on no desk, the called desk is not an ancestor.
    assert!(
        !channel_call_target::in_ancestry_on(&mut db, claim.fence.scope, s.desk)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn channel_call_snapshot_intake_refused_before_write() {
    let s = Box::pin(CallSetup::with(destination(Some(json!({
        "type":"object",
        "properties":{"order":{"type":"string","description":"The order id"}}
    })))))
    .await;
    let claim = s.claim("call-v12").await;
    let invalid = json!({"channel_id": s.desk.to_string(), "content": {"body": "x"}, "fields": {"order": 42}});
    let CallStart::Answered(value) = s.start(&claim, &invalid).await.unwrap() else {
        panic!("expected an intake error");
    };
    assert_eq!(value["error"]["code"], INTAKE_INVALID);
    assert!(
        value["error"]["schema"].is_object() && value["error"]["paths"].is_array(),
        "{value}"
    );
    assert_no_call_rows(&s).await;
    // A desk without an intake refuses fields.
    let plain = CallSetup::new().await;
    let claim = plain.claim("call-v12b").await;
    let fields = json!({"channel_id": plain.desk.to_string(), "content": {"body": "x"}, "fields": {"order": "42"}});
    assert_eq!(
        answered(plain.start(&claim, &fields).await.unwrap()),
        INTAKE_NOT_DECLARED
    );
    assert_no_call_rows(&plain).await;
}

#[tokio::test]
async fn channel_call_snapshot_audience_refused_above_ceiling() {
    let s = CallSetup::new().await;
    let claim = s.claim("call-v13").await;
    sqlx::query("UPDATE workflow_runs SET content_level=1 WHERE id=$1 AND knowledge_ceiling>=1")
        .bind(claim.fence.scope.run.as_uuid())
        .execute(s.pool())
        .await
        .unwrap();
    assert_eq!(
        answered(s.start(&claim, &s.arguments("Classified")).await.unwrap()),
        AUDIENCE_REFUSED
    );
    assert_no_call_rows(&s).await;
}

#[tokio::test]
async fn channel_call_snapshot_role_refusal_is_denied() {
    let s = Box::pin(CallSetup::with(role_destination())).await;
    // Nobody holds the reviewer role on the desk's unit chain, so capture refuses it.
    let claim = s.claim("call-v14").await;
    let mut tx = s.pool().begin().await.unwrap();
    assert_eq!(
        answered(
            s.start_in(&mut tx, &claim, &s.arguments("Roles"))
                .await
                .unwrap()
        ),
        DESTINATION_UNAVAILABLE
    );
    // The savepoint undid the child; the caller's fence is still held by this transaction.
    let children: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_runs WHERE parent_run_id=$1")
            .bind(claim.fence.scope.run.as_uuid())
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(children, 0);
    assert!(
        lease::lock_fence(&mut tx, claim.fence, std::time::Duration::from_secs(1))
            .await
            .unwrap()
    );
    tx.commit().await.unwrap();
    assert_no_call_rows(&s).await;
}

#[tokio::test]
async fn channel_call_snapshot_unit_cap_is_denied() {
    let s = CallSetup::new().await;
    // The caller runs from a desk in a sibling unit of the called desk's, so the two cap chains
    // share only the root.
    let caller_unit = s.p().test_unit(s.company().as_uuid(), "caller-unit").await;
    let caller_desk = new_channel(&s.tool.fixture, "caller-desk").await;
    sqlx::query("UPDATE channels SET department_id=$2 WHERE id=$1")
        .bind(caller_desk)
        .bind(caller_unit)
        .execute(s.pool())
        .await
        .unwrap();
    let within = s.claim_from(caller_desk, "call-v15-a").await;
    let over = s.claim_from(caller_desk, "call-v15-b").await;
    assert_eq!(s.write_unit(within.fence.scope.run).await, caller_unit);
    // A cap on the caller's unit alone does not refuse: the child charges the desk's chain.
    s.exhaust(caller_unit).await;
    let child = started(
        s.start(&within, &s.arguments("Caller over cap"))
            .await
            .unwrap(),
    );
    assert_eq!(s.write_unit(child).await, s.desk_unit().await);
    // A cap on the desk's unit refuses, under the savepoint, with the caller's fence still held.
    s.exhaust(s.desk_unit().await).await;
    let mut tx = s.pool().begin().await.unwrap();
    assert_eq!(
        answered(
            s.start_in(&mut tx, &over, &s.arguments("Desk over cap"))
                .await
                .unwrap()
        ),
        crate::app_error::BudgetExhausted::CODE
    );
    assert!(
        lease::lock_fence(&mut tx, over.fence, std::time::Duration::from_secs(1))
            .await
            .unwrap()
    );
    tx.commit().await.unwrap();
    assert_eq!(s.records().await, 1);
    let refused_children: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_runs WHERE parent_run_id=$1")
            .bind(over.fence.scope.run.as_uuid())
            .fetch_one(s.pool())
            .await
            .unwrap();
    assert_eq!(refused_children, 0);
}

/// F8: what the child writer's savepoint turns into a denied closure, and what it propagates.
#[test]
fn channel_call_snapshot_destination_refusals_are_classified() {
    for refusal in [
        AppError::BadRequest(
            "Agent dependency graph is unavailable, cyclic or exceeds its limits".into(),
        ),
        AppError::NotFound("agent".into()),
        AppError::Forbidden("grant".into()),
        AppError::UnitArchived,
    ] {
        assert!(destination_refusal(&refusal), "{refusal:?}");
    }
    for propagated in [
        AppError::Timeout("capture".into()),
        AppError::Database("down".into()),
        AppError::Conflict("settings".into()),
        AppError::Internal("defect".into()),
    ] {
        assert!(!destination_refusal(&propagated), "{propagated:?}");
    }
}

// ---------------------------------------------------------------------------------------------
// V7–V10 — the explicit dependency scope.

/// A bare run under the caller, written as the child writer writes one but with no scope yet,
/// so each test can attach exactly the scope it wants to prove.
async fn bare_child(
    s: &CallSetup,
    tx: &mut Transaction<'_, Postgres>,
    caller: ActivationRequest,
) -> RunId {
    let state = super::super::binding_rows::read_binding_at(tx, s.company(), s.responder, None)
        .await
        .unwrap()
        .unwrap();
    let run = RunId::new(Uuid::new_v4());
    super::super::admission_write::insert_run(
        tx,
        &super::super::admission_write::RunRow {
            goal: None,
            company: s.company(),
            knowledge: crate::entities::access_level::RunKnowledge::new(
                crate::entities::access_level::AccessLevel::PUBLIC,
                crate::entities::access_level::AccessLevel::PUBLIC,
            )
            .unwrap(),
            content_exact: true,
            run,
            binding: &state.configuration,
            association: RelatedAssociation::inbox(crate::entities::address::AddressTarget::Desk(
                s.desk,
            )),
            actor_id: s.run_as,
            trigger: TriggerId::new(Uuid::new_v4()),
            correlation: Uuid::new_v4(),
            input: &json!({}),
            deadline: super::super::admission_write::RunDeadline::AdmissionWindow,
            parent: Some(super::super::admission_write::ParentLink {
                run: caller.run,
                execution: caller.execution,
            }),
            rerun_of: None,
            delivery_policy: crate::application::workflow::DeliveryPolicy::Deliver,
            native_mcp_call: None,
        },
    )
    .await
    .unwrap();
    run
}

async fn scope_of(tx: &mut Transaction<'_, Postgres>, run: RunId) -> (Uuid, Uuid) {
    sqlx::query_as("SELECT root_run_id, manifest_id FROM run_dependency_manifests WHERE run_id=$1")
        .bind(run.as_uuid())
        .fetch_one(&mut **tx)
        .await
        .unwrap()
}

async fn attach(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    run: RunId,
    scope: (Uuid, Uuid),
) {
    sqlx::query("INSERT INTO run_dependency_manifests(company_id,run_id,root_run_id,manifest_id) VALUES($1,$2,$3,$4)")
        .bind(company.as_uuid())
        .bind(run.as_uuid())
        .bind(scope.0)
        .bind(scope.1)
        .execute(&mut **tx)
        .await
        .unwrap();
}

#[tokio::test]
async fn channel_call_manifest_call_scope_requires_invocation() {
    let s = CallSetup::new().await;
    let claim = s.claim("call-v7").await;
    let mut tx = s.pool().begin().await.unwrap();
    let run = bare_child(&s, &mut tx, claim.fence.scope).await;
    let state =
        super::super::binding_rows::read_binding_at(&mut tx, s.company(), s.responder, None)
            .await
            .unwrap()
            .unwrap();
    // A fresh scope captured for the bare child, exactly as a call child's, but with no record.
    let manifest = super::super::agent_manifest::capture_on(
        &mut tx,
        super::super::agent_manifest::CaptureScope {
            company: s.company(),
            actor: WorkflowActor::authenticated(s.run_as).unwrap(),
            binding: &state.configuration,
            association: RelatedAssociation::inbox(crate::entities::address::AddressTarget::Desk(
                s.desk,
            )),
            scope_root: run,
        },
    )
    .await
    .unwrap();
    super::super::agent_manifest::persist_on(&mut tx, &manifest)
        .await
        .unwrap();
    assert_eq!(
        immediate(&mut tx, "run_dependency_scope").await,
        Err("run_dependency_scope".to_owned())
    );
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn channel_call_manifest_child_cannot_take_sibling_or_foreign_scope() {
    let s = CallSetup::new().await;
    let claim = s.claim("call-v8").await;
    let foreign = s.claim("call-v8-foreign").await;
    let mut tx = s.pool().begin().await.unwrap();
    let sibling = started(
        s.start_in(&mut tx, &claim, &s.arguments("Sibling"))
            .await
            .unwrap(),
    );
    let sibling_scope = scope_of(&mut tx, sibling).await;
    let foreign_scope = scope_of(&mut tx, foreign.fence.scope.run).await;
    let parent_scope = scope_of(&mut tx, claim.fence.scope.run).await;
    for (case, scope, refused) in [
        ("the sibling call child's scope", sibling_scope, true),
        ("another tree's scope", foreign_scope, true),
        ("its parent's scope", parent_scope, false),
    ] {
        sqlx::query("SAVEPOINT scope")
            .execute(&mut *tx)
            .await
            .unwrap();
        let run = bare_child(&s, &mut tx, claim.fence.scope).await;
        attach(&mut tx, s.company(), run, scope).await;
        let expected = if refused {
            Err("run_dependency_scope".to_owned())
        } else {
            Ok(())
        };
        assert_eq!(
            immediate(&mut tx, "run_dependency_scope").await,
            expected,
            "{case}"
        );
        sqlx::query("ROLLBACK TO SAVEPOINT scope")
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn channel_call_manifest_pins_survive_publication() {
    let s = CallSetup::new().await;
    let claim = s.claim("call-v9").await;
    let child = started(s.start(&claim, &s.arguments("Pinned")).await.unwrap());
    // A newer destination version is published after the capture.
    let fixture = &s.tool.fixture;
    let workflow = WorkflowId::new(Uuid::from_u128(DESTINATION));
    let target = DefinitionTarget {
        workflow,
        ..fixture.target
    };
    let mut source = destination(None);
    source["workflow_id"] = json!(workflow.to_string());
    let mut draft = fixture.save(Some(DraftRevision::new(1).unwrap()));
    draft.target = target;
    draft.content.source = source.to_string();
    let service = fixture.service();
    service.save(draft).await.unwrap();
    let mut publication = fixture.publish();
    publication.target = target;
    publication.expected = DraftRevision::new(2).unwrap();
    publication.key = IdempotencyKey::parse("republication").unwrap();
    let newer = service.publish(publication).await.unwrap();
    assert_ne!(newer.compiled().graph().definition().version_id, s.version);
    let selected: Vec<Uuid> = sqlx::query_scalar(
        "SELECT selection.version_id FROM run_dependency_manifests AS ownership \
         JOIN agent_manifest_workflows AS selection ON selection.company_id=ownership.company_id \
           AND selection.manifest_id=ownership.manifest_id WHERE ownership.run_id=$1",
    )
    .bind(child.as_uuid())
    .fetch_all(s.pool())
    .await
    .unwrap();
    assert_eq!(selected, vec![s.version.as_uuid()]);
}

#[tokio::test]
async fn channel_call_manifest_descendant_inherits_call_scope() {
    let s = CallSetup::new().await;
    let claim = s.claim("call-v10").await;
    let mut tx = s.pool().begin().await.unwrap();
    let child = started(
        s.start_in(&mut tx, &claim, &s.arguments("Nested"))
            .await
            .unwrap(),
    );
    let call_scope = scope_of(&mut tx, child).await;
    let caller_scope = scope_of(&mut tx, claim.fence.scope.run).await;
    let (execution, job): (Uuid, Uuid) = sqlx::query_as(
        "SELECT execution.id, job.id FROM workflow_executions AS execution \
         JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id \
         WHERE execution.run_id=$1",
    )
    .bind(child.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let parent = ActivationRequest {
        company: s.company(),
        run: child,
        execution: ExecutionId::new(execution),
        job: crate::application::workflow::activation::WorkflowJobId(job),
    };
    let state =
        super::super::binding_rows::read_binding_at(&mut tx, s.company(), s.responder, None)
            .await
            .unwrap()
            .unwrap();
    let step = StepId::parse("answer").unwrap();
    super::super::children::create_on(
        &mut tx,
        parent,
        &super::super::children::ChildStart {
            step: &step,
            origin: super::super::children::ChildOrigin::Round(
                crate::domain::workflow::ChildOrdinal::FIRST,
            ),
            binding: &state.configuration,
            input: &json!({}),
            deadline: Utc::now() + chrono::Duration::minutes(5),
        },
    )
    .await
    .unwrap();
    let grandchild: Uuid =
        sqlx::query_scalar("SELECT id FROM workflow_runs WHERE parent_run_id=$1")
            .bind(child.as_uuid())
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    let inherited = scope_of(&mut tx, RunId::new(grandchild)).await;
    assert_eq!(inherited, call_scope);
    assert_ne!(inherited, caller_scope);
    assert_eq!(immediate(&mut tx, "run_dependency_scope").await, Ok(()));
    tx.rollback().await.unwrap();
}
