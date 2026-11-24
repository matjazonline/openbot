//! `goal_trail_` (Plan08 03B, gate 08G-P03-D1 r2): every run done on a goal's behalf records it,
//! through `channel.call`'s child and thread, a later reply, an inherited child and a rerun; the
//! column is fixed and names live goals only; a goal with recorded work is archived, not deleted.
use super::channel_call_support::*;
use super::workflow_tool_support::*;
use super::*;
use crate::application::ports::department::OpenWork;
use crate::application::workflow::agent::AgentToolObservation;
use crate::application::workflow::channel_reply::ReplySettingsRevision;
use crate::application::workflow::children::WorkflowChildren;
use crate::domain::entities::correlation::CorrelationId;
use crate::entities::department::DepartmentId;
use serde_json::json;

const SYSTEM: &str = r#"{"actor_type":"system","actor_id":null,"actor_name":"Fixture"}"#;

/// A goal unit under the root with an enabled public desk of its own.
pub(super) async fn goal_with_desk(s: &CallSetup, name: &str) -> (Uuid, Uuid) {
    let goal = s.p().test_unit(s.company().as_uuid(), name).await;
    sqlx::query("UPDATE departments SET kind = 'goal' WHERE id = $1")
        .bind(goal)
        .execute(s.pool())
        .await
        .unwrap();
    let desk = new_channel(&s.tool.fixture, &format!("{name}-desk")).await;
    sqlx::query(
        "UPDATE channels SET enabled = true, access_mode = 'public', department_id = $2 WHERE id = $1",
    )
    .bind(desk)
    .bind(goal)
    .execute(s.pool())
    .await
    .unwrap();
    (goal, desk)
}

pub(super) async fn run_goal(s: &CallSetup, run: Uuid) -> Option<Uuid> {
    sqlx::query_scalar("SELECT goal_unit_id FROM workflow_runs WHERE id = $1")
        .bind(run)
        .fetch_one(s.pool())
        .await
        .unwrap()
}

async fn thread_goal(s: &CallSetup, thread: Uuid) -> Option<Uuid> {
    sqlx::query_scalar("SELECT goal_unit_id FROM threads WHERE id = $1")
        .bind(thread)
        .fetch_one(s.pool())
        .await
        .unwrap()
}

/// Mark a unit archived directly, to test what resolution does with it.
pub(super) async fn archive_raw(s: &CallSetup, unit: Uuid) {
    sqlx::query(
        "UPDATE departments SET archived_at = now(), archived_by = $2::jsonb WHERE id = $1",
    )
    .bind(unit)
    .bind(SYSTEM)
    .execute(s.pool())
    .await
    .unwrap();
}

/// Admit a manual run of the caller's binding on `association`, optionally as a rerun.
async fn admit(
    s: &CallSetup,
    association: RelatedAssociation,
    key: &str,
    rerun_of: Option<RunId>,
) -> RunId {
    match try_admit(s, association, key, rerun_of).await.unwrap() {
        AdmissionResult::Created(run) => run,
        other => panic!("{other:?}"),
    }
}

/// The same admission, returning its refusal.
pub(super) async fn try_admit(
    s: &CallSetup,
    association: RelatedAssociation,
    key: &str,
    rerun_of: Option<RunId>,
) -> AppResult<AdmissionResult> {
    let p = s.p();
    let company = s.company();
    let admission = WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight)
        .admit(AdmitWorkflowRequest {
            history_session: None,
            company_id: company,
            actor: s.owner(),
            association,
            trigger: TriggerRef::new(
                company,
                TriggerId::new(Uuid::new_v4()),
                TriggerSource::Manual,
            )
            .unwrap(),
            correlation_id: CorrelationId::new(),
            binding_id: s.tool.target.binding,
            idempotency_key: IdempotencyKey::parse(key).unwrap(),
            input: json!({}),
            rerun_of,
            delivery_policy: crate::application::workflow::DeliveryPolicy::Deliver,
            expected_binding_revision: None,
            admission_claim: None,
        })
        .await?;
    Ok(admission)
}

pub(super) fn on_thread(desk: Uuid, thread: Uuid) -> RelatedAssociation {
    RelatedAssociation::Thread {
        target: crate::entities::address::AddressTarget::Desk(desk),
        thread_id: RelatedThreadId::new(thread),
    }
}

/// The call's child and its fresh thread carry the caller's goal; a later reply on that thread
/// carries it through the thread; a rerun keeps it; a run on an unrelated desk serves none; once
/// the goal is archived the same reply is admitted by its department and serves no goal.
#[tokio::test]
async fn goal_trail_call_child_thread_reply_and_rerun_carry_the_goal() {
    let s = CallSetup::new().await;
    let (goal, front) = goal_with_desk(&s, "launch").await;
    let caller = s.claim_from(front, "goal-trail-caller").await;
    let caller_run = caller.fence.scope.run;
    assert_eq!(
        run_goal(&s, caller_run.as_uuid()).await,
        Some(goal),
        "the desk's goal"
    );
    assert!(matches!(
        s.call(&caller, &s.arguments("Plan the launch"))
            .await
            .unwrap(),
        AgentToolObservation::Parked(_)
    ));
    let record = s.record(caller_run).await;
    assert_eq!(
        run_goal(&s, record.child_run_id).await,
        Some(goal),
        "the call child"
    );
    assert_eq!(
        thread_goal(&s, record.thread_id).await,
        Some(goal),
        "the call's thread"
    );
    let reply = admit(
        &s,
        on_thread(s.desk, record.thread_id),
        "goal-trail-reply",
        None,
    )
    .await;
    assert_eq!(
        run_goal(&s, reply.as_uuid()).await,
        Some(goal),
        "a later reply on the thread"
    );
    let rerun = admit(
        &s,
        RelatedAssociation::Company,
        "goal-trail-rerun",
        Some(caller_run),
    )
    .await;
    assert_eq!(
        run_goal(&s, rerun.as_uuid()).await,
        Some(goal),
        "a rerun keeps it"
    );
    let plain = s.claim("goal-trail-plain").await;
    assert_eq!(
        run_goal(&s, plain.fence.scope.run.as_uuid()).await,
        None,
        "an unrelated run"
    );
    archive_raw(&s, goal).await;
    let late = admit(
        &s,
        on_thread(s.desk, record.thread_id),
        "goal-trail-late",
        None,
    )
    .await;
    assert_eq!(
        run_goal(&s, late.as_uuid()).await,
        None,
        "a closed goal records no new work"
    );
}

/// An ordinary child (`workflow.call`) inherits its parent's goal: here the call child's own
/// static child, two levels below the goal's run (S3).
#[tokio::test]
async fn goal_trail_inherited_child_carries_the_parent_goal() {
    let s = CallSetup::new().await;
    let leaf_id = WorkflowId::new(Uuid::new_v4());
    let leaf = s
        .tool
        .fixture
        .publish_workflow(leaf_id, destination(None))
        .await;
    let workflow = WorkflowId::new(Uuid::new_v4());
    let mut source = destination(None);
    source["workflow_id"] = json!(workflow.as_uuid());
    source["steps"]["answer"] = json!({"type":"workflow.call",
        "child_workflow_id":leaf_id.to_string(),"child_version_id":leaf.to_string(),
        "with":{"input":{"literal":{}}},"routes":{"success":"$end"}});
    let delegating = publish_agent_workflow(
        &s.tool.fixture,
        workflow,
        "goal-trail-static-destination",
        source.to_string(),
        step_builtins::StepBuiltinPublicationDirectory {
            persistence: s.p().clone(),
        },
    )
    .await
    .unwrap();
    let binding = s
        .responder_on(s.desk, delegating, s.run_as, json!({}))
        .await;
    s.select_with(
        s.desk,
        binding,
        Some(ReplySettingsRevision::try_from(1).unwrap()),
        None,
        literal("Delegated"),
    )
    .await
    .unwrap();
    let (goal, front) = goal_with_desk(&s, "inherit").await;
    let caller = s.claim_from(front, "goal-trail-inherit").await;
    s.call(&caller, &s.arguments("Delegate it")).await.unwrap();
    let child = s.tool.child_of(caller.fence.scope).await;
    s.p().park_child(child).await.unwrap().unwrap();
    let grandchild = s.tool.child_of(child).await;
    assert_eq!(run_goal(&s, child.run.as_uuid()).await, Some(goal));
    assert_eq!(
        run_goal(&s, grandchild.run.as_uuid()).await,
        Some(goal),
        "the inherited child"
    );
}

/// A goal that becomes standing keeps its recorded work, and new work is admitted serving no goal
/// instead of failing: the resolver and the insert guard read the same facts (D1 r2, F1).
#[tokio::test]
async fn goal_trail_kind_change_keeps_history_and_admits_new_work() {
    let s = CallSetup::new().await;
    let (goal, front) = goal_with_desk(&s, "pivot").await;
    let first = s.claim_from(front, "goal-trail-pivot-first").await;
    let second = s.claim_from(front, "goal-trail-pivot-second").await;
    s.call(&first, &s.arguments("Before")).await.unwrap();
    let before = s.record(first.fence.scope.run).await;
    sqlx::query("UPDATE departments SET kind = 'standing' WHERE id = $1")
        .bind(goal)
        .execute(s.pool())
        .await
        .unwrap();
    assert_eq!(
        run_goal(&s, first.fence.scope.run.as_uuid()).await,
        Some(goal)
    );
    assert_eq!(run_goal(&s, before.child_run_id).await, Some(goal));
    assert_eq!(
        thread_goal(&s, before.thread_id).await,
        Some(goal),
        "history stays"
    );
    // The second caller recorded the goal; its call now records none, and is still admitted.
    assert_eq!(
        run_goal(&s, second.fence.scope.run.as_uuid()).await,
        Some(goal)
    );
    assert!(matches!(
        s.call(&second, &s.arguments("After")).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    let after = s.record(second.fence.scope.run).await;
    assert_eq!(run_goal(&s, after.child_run_id).await, None);
    assert_eq!(thread_goal(&s, after.thread_id).await, None);
    let new = s.claim_from(front, "goal-trail-pivot-new").await;
    assert_eq!(run_goal(&s, new.fence.scope.run.as_uuid()).await, None);
}

/// The column is fixed once written and names this company's live goals only; a goal with
/// recorded work refuses deletion, by its guard and by its keys; a company teardown still removes
/// everything (D1.3, F5, S2).
#[tokio::test]
async fn goal_trail_column_is_immutable_and_goal_only() {
    let s = CallSetup::new().await;
    let (goal, front) = goal_with_desk(&s, "fixed").await;
    let caller = s.claim_from(front, "goal-trail-fixed").await;
    s.call(&caller, &s.arguments("Fixed")).await.unwrap();
    let record = s.record(caller.fence.scope.run).await;
    for sql in [
        "UPDATE workflow_runs SET goal_unit_id = NULL WHERE id = $1",
        "UPDATE threads SET goal_unit_id = NULL WHERE id = $1",
    ] {
        let id = if sql.contains("threads") {
            record.thread_id
        } else {
            record.child_run_id
        };
        let error = sqlx::query(sql)
            .bind(id)
            .execute(s.pool())
            .await
            .unwrap_err();
        assert_eq!(refused_by(error), "goal_unit_fixed", "{sql}");
    }
    let insert_thread = |unit: Uuid| {
        sqlx::query(
            "INSERT INTO threads (id, company_id, channel_id, subject, goal_unit_id) \
             VALUES (gen_random_uuid(), $1, $2, 'forged', $3)",
        )
        .bind(s.company().as_uuid())
        .bind(s.desk)
        .bind(unit)
    };
    let standing = s.desk_unit().await;
    let error = insert_thread(standing).execute(s.pool()).await.unwrap_err();
    assert_eq!(refused_by(error), "goal_unit_live", "a standing unit");
    let (closed, _) = goal_with_desk(&s, "closed").await;
    archive_raw(&s, closed).await;
    let error = insert_thread(closed).execute(s.pool()).await.unwrap_err();
    assert_eq!(refused_by(error), "goal_unit_live", "an archived goal");
    // Another company's goal unit fails the composite keys on both tables.
    let foreign_desk = foreign_channel(&s).await;
    let foreign_company: Uuid = sqlx::query_scalar("SELECT company_id FROM channels WHERE id = $1")
        .bind(foreign_desk)
        .fetch_one(s.pool())
        .await
        .unwrap();
    let foreign = s.p().test_unit(foreign_company, "foreign-goal").await;
    sqlx::query("UPDATE departments SET kind = 'goal' WHERE id = $1")
        .bind(foreign)
        .execute(s.pool())
        .await
        .unwrap();
    let error = insert_thread(foreign).execute(s.pool()).await.unwrap_err();
    assert_eq!(refused_by(error), "threads_goal_fk");
    let error = sqlx::query(
        "INSERT INTO workflow_runs SELECT (jsonb_populate_record(NULL::workflow_runs, \
           to_jsonb(run) || jsonb_build_object('id', gen_random_uuid(), 'goal_unit_id', $2::uuid))).* \
           FROM workflow_runs AS run WHERE run.id = $1",
    )
    .bind(record.child_run_id)
    .bind(foreign)
    .execute(s.pool())
    .await
    .unwrap_err();
    assert_eq!(refused_by(error), "workflow_runs_goal_fk");
    // Deleting the goal: the guard words it, and the keys refuse it below the guard.
    let departments = crate::adapters::persistence::test_support::department_use_cases(s.p());
    let refused = departments
        .delete_department(
            s.owner().user_id(),
            s.company().as_uuid(),
            DepartmentId::new(goal),
        )
        .await;
    assert!(
        matches!(&refused, Err(AppError::Conflict(message)) if message.contains("Archive it instead")),
        "{refused:?}"
    );
    // Move the goal's desk out, so only the recorded goal holds the unit.
    sqlx::query("UPDATE channels SET department_id = $2 WHERE id = $1")
        .bind(front)
        .bind(standing)
        .execute(s.pool())
        .await
        .unwrap();
    let error = sqlx::query("DELETE FROM departments WHERE id = $1")
        .bind(goal)
        .execute(s.pool())
        .await
        .unwrap_err();
    assert!(
        ["workflow_runs_goal_fk", "threads_goal_fk"].contains(&refused_by(error).as_str()),
        "RESTRICT holds below the guard"
    );
    // A company teardown removes the runs and threads before the tree.
    crate::application::use_cases::company::CompanyPersistence::delete(
        s.p(),
        s.company().as_uuid(),
    )
    .await
    .unwrap();
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM departments WHERE company_id = $1")
        .bind(s.company().as_uuid())
        .fetch_one(s.pool())
        .await
        .unwrap();
    assert_eq!(left, 0);
}

/// Cancel-and-archive reaches a goal's work elsewhere through the trees it cancels: a goal root
/// with an open call child in another department is cancellable, and both end (D1 r2, F3).
#[tokio::test]
async fn goal_trail_archive_cancels_goal_tree_with_call_child_elsewhere() {
    let s = CallSetup::new().await;
    let (goal, front) = goal_with_desk(&s, "wrap").await;
    let caller = s.claim_from(front, "goal-trail-wrap").await;
    s.call(&caller, &s.arguments("Wrap up")).await.unwrap();
    let record = s.record(caller.fence.scope.run).await;
    let departments = crate::adapters::persistence::test_support::department_use_cases(s.p());
    let user = s.owner().user_id();
    let company = s.company().as_uuid();
    let refused = departments
        .archive_department(user, company, DepartmentId::new(goal), OpenWork::Refuse)
        .await;
    assert!(
        matches!(
            refused,
            Err(AppError::UnitArchiveOpenWork {
                count: 2,
                elsewhere: 0,
                cancellable: true
            })
        ),
        "{refused:?}"
    );
    let archived = departments
        .archive_department(user, company, DepartmentId::new(goal), OpenWork::Cancel)
        .await
        .expect("the goal's tree, with its call child elsewhere, is cancelled");
    assert!(archived.is_archived());
    for run in [caller.fence.scope.run.as_uuid(), record.child_run_id] {
        assert_eq!(run_state(s.p(), RunId::new(run)).await, "cancelled");
    }
}
