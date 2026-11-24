//! `goal_budget_`, `goal_competing_` (budget half) and `goal_rollup_` (Plan08 03C–03D, gate
//! 08G-P03-D2 r2): a goal's `total` cap counts the work done for it in every department, refuses
//! new work for it at admission and at a `channel.call` child, and the goal's rollup counts its
//! runs, calls and spend from the same rows.
use super::channel_call_support::*;
use super::goal_trail_tests::{archive_raw, goal_with_desk, on_thread, try_admit};
use super::workflow_tool_support::kind;
use super::*;
use crate::adapters::persistence::department::goal_rollup::goal_rollup_on;
use crate::adapters::persistence::test_support::wait_until_backends_are_blocked;
use crate::application::ports::department::{DepartmentEdit, GOAL_ROLLUP_RUN_LIMIT, GoalRollup};
use crate::application::workflow::agent::AgentToolObservation;
use crate::application::workflow::goal_status::{
    GOAL_NONE, GOAL_NOT_AUTHORIZED, GoalStatusDispatch, GoalStatusToolCall,
};
use crate::entities::department::{DepartmentId, DepartmentKind};
use crate::services::test_support::{LlmTurn, register_scripted_agent_base_url, scripted_llm};
use crate::use_cases::company::CompanyPersistence;
use serde_json::json;

/// One micro-USD per input token for this model; any other model is unpriced.
const PRICED: &str = "goal-fixture";

async fn price(s: &CallSetup) {
    sqlx::query(
        "INSERT INTO model_prices(company_id, provider, model, input_micro_usd_per_million, \
         output_micro_usd_per_million) VALUES ($1, 'openai', $2, 1000000, 1000000)",
    )
    .bind(s.company().as_uuid())
    .bind(PRICED)
    .execute(s.pool())
    .await
    .unwrap();
}

/// A reserved model call of `run` (`unit` overrides its attribution), returning its id.
async fn reserve(s: &CallSetup, run: Uuid, model: &str, unit: Option<Uuid>) -> Uuid {
    let mut row: serde_json::Value = sqlx::query_scalar(
        "SELECT jsonb_build_object('company_id',run.company_id,'request_id',gen_random_uuid(), \
           'task_id',task.id,'agent_id',$3::uuid,'workflow_id',run.workflow_id, \
           'version_id',run.version_id,'run_id',run.id,'execution_id',task.workflow_execution_id, \
           'root_run_id',budget.root_run_id,'provider','openai','model',$2::text, \
           'reserved_input',100,'reserved_output',50,'requested_at',statement_timestamp()) \
         FROM workflow_runs AS run \
         JOIN workflow_executions AS execution \
           ON execution.company_id=run.company_id AND execution.run_id=run.id \
         JOIN background_tasks AS task \
           ON task.company_id=run.company_id AND task.workflow_execution_id=execution.id \
         JOIN workflow_run_budgets AS budget \
           ON budget.company_id=run.company_id AND budget.run_id=run.id \
         WHERE run.id=$1 LIMIT 1",
    )
    .bind(run)
    .bind(model)
    .bind(s.tool.agent)
    .fetch_one(s.pool())
    .await
    .unwrap();
    if let Some(unit) = unit {
        row["unit_id"] = json!(unit);
    }
    // A writer cannot choose the goal: the trigger takes the run's.
    row["goal_unit_id"] = json!(Uuid::new_v4());
    sqlx::query(
        "INSERT INTO model_usage_calls SELECT (jsonb_populate_record(NULL::model_usage_calls,$1)).*",
    )
    .bind(&row)
    .execute(s.pool())
    .await
    .unwrap();
    serde_json::from_value(row["request_id"].clone()).unwrap()
}

async fn settle(s: &CallSetup, request: Uuid, tokens: i64) {
    sqlx::query(
        "UPDATE model_usage_calls SET input_tokens=$3, output_tokens=0, usage_source='reported', \
         settled_at=statement_timestamp() WHERE company_id=$1 AND request_id=$2",
    )
    .bind(s.company().as_uuid())
    .bind(request)
    .bind(tokens)
    .execute(s.pool())
    .await
    .unwrap();
}

/// Charge `micros` of priced spend to `run` (one settled call).
async fn charge(s: &CallSetup, run: Uuid, micros: i64) {
    let call = reserve(s, run, PRICED, None).await;
    settle(s, call, micros).await;
}

/// The goal's counter rows: (unit, spent, unpriced), by unit.
async fn goal_rows(s: &CallSetup, goal: Uuid) -> Vec<(Uuid, i64, i64)> {
    sqlx::query_as(
        "SELECT unit_id, spent_micros, unpriced_tokens FROM goal_spend \
          WHERE company_id=$1 AND goal_unit_id=$2 ORDER BY unit_id",
    )
    .bind(s.company().as_uuid())
    .bind(goal)
    .fetch_all(s.pool())
    .await
    .unwrap()
}

/// The same totals computed from the settled calls themselves.
async fn goal_rows_from_calls(s: &CallSetup, goal: Uuid) -> Vec<(Uuid, i64, i64)> {
    sqlx::query_as(
        "SELECT call.unit_id, \
                COALESCE(sum(CASE WHEN price.model IS NULL THEN 0 ELSE \
                  ceil((call.input_tokens * price.input_micro_usd_per_million \
                      + call.output_tokens * price.output_micro_usd_per_million) / 1000000.0) END), 0)::bigint, \
                COALESCE(sum(CASE WHEN price.model IS NULL \
                  THEN call.input_tokens + call.output_tokens ELSE 0 END), 0)::bigint \
           FROM model_usage_calls AS call \
           LEFT JOIN model_prices AS price ON price.company_id=call.company_id \
            AND price.provider=call.provider AND price.model=call.model \
          WHERE call.company_id=$1 AND call.goal_unit_id=$2 AND call.settled_at IS NOT NULL \
          GROUP BY call.unit_id ORDER BY call.unit_id",
    )
    .bind(s.company().as_uuid())
    .bind(goal)
    .fetch_all(s.pool())
    .await
    .unwrap()
}

async fn cap(s: &CallSetup, unit: Uuid, period: &str, micros: i64) -> AppResult<()> {
    CompanyPersistence::set_unit_budget(
        s.p(),
        s.owner().user_id(),
        s.company().as_uuid(),
        unit,
        period,
        micros,
    )
    .await
}

fn goal_exhausted<T: std::fmt::Debug>(result: &AppResult<T>, goal: Uuid, name: &str) -> bool {
    matches!(result, Err(AppError::BudgetExhausted(refusal))
        if refusal.unit == goal && refusal.period == "total"
            && refusal.code() == "budget.goal_exhausted"
            && refusal.to_string().contains(name))
}

fn bad_request<T: std::fmt::Debug>(result: &AppResult<T>, code: &str) -> bool {
    matches!(result, Err(AppError::BadRequest(message)) if message.starts_with(code))
}

fn constraint(error: sqlx::Error) -> String {
    error
        .as_database_error()
        .and_then(|error| error.constraint())
        .unwrap_or_default()
        .to_owned()
}

/// A unit under `parent` with an enabled public desk of its own.
async fn unit_with_desk(s: &CallSetup, parent: Uuid, name: &str) -> (Uuid, Uuid) {
    let unit: Uuid = sqlx::query_scalar(
        "INSERT INTO departments (id, company_id, parent_id, name, slug, created_by) \
         VALUES (gen_random_uuid(), $1, $2, $3, $3, \
         '{\"actor_type\":\"system\",\"actor_id\":null,\"actor_name\":\"Fixture\"}') RETURNING id",
    )
    .bind(s.company().as_uuid())
    .bind(parent)
    .bind(name)
    .fetch_one(s.pool())
    .await
    .unwrap();
    let desk = new_channel(&s.tool.fixture, &format!("{name}-desk")).await;
    sqlx::query(
        "UPDATE channels SET enabled=true, access_mode='public', department_id=$2 WHERE id=$1",
    )
    .bind(desk)
    .bind(unit)
    .execute(s.pool())
    .await
    .unwrap();
    (unit, desk)
}

/// `desk` answers channel calls with the destination workflow.
async fn answering(s: &CallSetup, desk: Uuid) {
    let binding = s.responder_on(desk, s.version, s.run_as, json!({})).await;
    s.select_on(desk, binding, None, None).await.unwrap();
}

fn to(desk: Uuid, body: &str) -> serde_json::Value {
    json!({"channel_id": desk.to_string(), "content": {"body": body}})
}

fn created(result: AppResult<AdmissionResult>) -> Uuid {
    match result.unwrap() {
        AdmissionResult::Created(run) => run.as_uuid(),
        other => panic!("{other:?}"),
    }
}

async fn rollup(s: &CallSetup, goal: Uuid, limit: i64) -> GoalRollup {
    let mut db = s.pool().acquire().await.unwrap();
    goal_rollup_on(&mut db, s.company(), DepartmentId::new(goal), limit)
        .await
        .unwrap()
}

/// 03C.1: `total` belongs to a goal (the app refuses it first, the trigger under the unit's
/// lock); a goal with a `total` cap stays a goal until the cap is cleared; day and month caps
/// keep their meaning on any unit.
#[tokio::test]
async fn goal_budget_total_cap_only_on_a_goal() {
    let s = CallSetup::new().await;
    let (goal, _) = goal_with_desk(&s, "launch").await;
    let standing = s.desk_unit().await;
    let (user, company) = (s.owner().user_id(), s.company().as_uuid());
    assert!(bad_request(
        &cap(&s, standing, "total", 100).await,
        "budget.total_goal_only"
    ));
    assert!(bad_request(
        &CompanyPersistence::replace_unit_budgets(
            s.p(),
            user,
            company,
            standing,
            None,
            None,
            Some(Some(100))
        )
        .await,
        "budget.total_goal_only"
    ));
    let raw = sqlx::query(
        "INSERT INTO unit_budgets (company_id, unit_id, period, cap_micros, created_by) \
         VALUES ($1, $2, 'total', 100, '{}'::jsonb)",
    )
    .bind(company)
    .bind(standing)
    .execute(s.pool())
    .await
    .unwrap_err();
    assert_eq!(constraint(raw), "unit_budgets_total_goal_only");
    cap(&s, standing, "day", 300).await.unwrap();
    cap(&s, goal, "total", 500).await.unwrap();
    cap(&s, goal, "month", 900).await.unwrap();
    let states = CompanyPersistence::unit_budget_states(s.p(), user, company, goal)
        .await
        .unwrap();
    let periods: Vec<(&str, Option<i64>)> = states
        .iter()
        .map(|state| (state.period, state.cap_micros))
        .collect();
    assert_eq!(
        periods,
        vec![("day", None), ("month", Some(900)), ("total", Some(500))]
    );
    let total = states.iter().find(|state| state.period == "total").unwrap();
    assert_eq!(
        (total.spent_micros, total.unpriced_tokens),
        (0, 0),
        "a goal with no spend has spent nothing"
    );
    let standing_states = CompanyPersistence::unit_budget_states(s.p(), user, company, standing)
        .await
        .unwrap();
    assert!(standing_states.iter().all(|state| state.period != "total"));
    let departments = crate::adapters::persistence::test_support::department_use_cases(s.p());
    let to_standing = || DepartmentEdit {
        kind: Some(DepartmentKind::Standing),
        ..DepartmentEdit::default()
    };
    let refused = departments
        .update_department(user, company, DepartmentId::new(goal), to_standing())
        .await;
    assert!(
        bad_request(&refused, "department.goal_budget_on_standing"),
        "{refused:?}"
    );
    let raw = sqlx::query("UPDATE departments SET kind='standing' WHERE id=$1")
        .bind(goal)
        .execute(s.pool())
        .await
        .unwrap_err();
    assert_eq!(constraint(raw), "departments_goal_budget");
    CompanyPersistence::clear_unit_budget(s.p(), user, company, goal, "total")
        .await
        .unwrap();
    departments
        .update_department(user, company, DepartmentId::new(goal), to_standing())
        .await
        .expect("without its total cap the goal may become standing");
    let caps: Vec<(Uuid, String, i64)> = sqlx::query_as(
        "SELECT unit_id, period, cap_micros FROM unit_budgets WHERE company_id=$1 \
          ORDER BY cap_micros",
    )
    .bind(company)
    .fetch_all(s.pool())
    .await
    .unwrap();
    assert_eq!(
        caps,
        vec![
            (standing, "day".to_owned(), 300),
            (goal, "month".to_owned(), 900)
        ],
        "day and month caps are untouched"
    );
}

/// 03C.1: a goal's counter adds the work done for it in each department (its own, a call child in
/// another, a call child in a third), per working unit, priced and unpriced; the call records the
/// run's goal whatever the writer sent; a run serving no goal charges no goal; unit totals are
/// charged as before.
#[tokio::test]
async fn goal_budget_spend_counts_work_in_every_department() {
    let s = CallSetup::new().await;
    price(&s).await;
    let (goal, front) = goal_with_desk(&s, "launch").await;
    let finance = s.desk_unit().await;
    let (ops, ops_desk) =
        unit_with_desk(&s, s.p().test_root_unit(s.company().as_uuid()).await, "ops").await;
    answering(&s, ops_desk).await;
    let first = s.claim_from(front, "goal-spend-first").await;
    let second = s.claim_from(front, "goal-spend-second").await;
    s.call(&first, &to(s.desk, "Budget")).await.unwrap();
    s.call(&second, &to(ops_desk, "Ops")).await.unwrap();
    let finance_child = s.record(first.fence.scope.run).await.child_run_id;
    let ops_child = s.record(second.fence.scope.run).await.child_run_id;
    let before = goal_rows(&s, goal).await;
    charge(&s, first.fence.scope.run.as_uuid(), 100).await;
    charge(&s, finance_child, 30).await;
    charge(&s, ops_child, 7).await;
    let unpriced = reserve(&s, finance_child, "unpriced-model", None).await;
    settle(&s, unpriced, 10).await;
    let plain = s.claim("goal-spend-plain").await;
    charge(&s, plain.fence.scope.run.as_uuid(), 5).await;
    let rows = goal_rows(&s, goal).await;
    let spent = |unit: Uuid, rows: &[(Uuid, i64, i64)]| {
        rows.iter()
            .find(|row| row.0 == unit)
            .map_or((0, 0), |row| (row.1, row.2))
    };
    let delta = |unit: Uuid| {
        let (after, unpriced) = spent(unit, &rows);
        let (was, was_unpriced) = spent(unit, &before);
        (after - was, unpriced - was_unpriced)
    };
    assert_eq!(delta(goal), (100, 0));
    assert_eq!(delta(finance), (30, 10));
    assert_eq!(delta(ops), (7, 0));
    assert_eq!(rows.len(), 3, "one row per working unit: {rows:?}");
    assert_eq!(
        rows,
        goal_rows_from_calls(&s, goal).await,
        "the counter equals the settled calls"
    );
    let goals: Vec<Option<Uuid>> = sqlx::query_scalar(
        "SELECT DISTINCT goal_unit_id FROM model_usage_calls WHERE company_id=$1 AND run_id=ANY($2)",
    )
    .bind(s.company().as_uuid())
    .bind(vec![first.fence.scope.run.as_uuid(), finance_child, ops_child])
    .fetch_all(s.pool())
    .await
    .unwrap();
    assert_eq!(goals, vec![Some(goal)], "the run's goal, not the writer's");
    let plain_goal: Vec<Option<Uuid>> = sqlx::query_scalar(
        "SELECT DISTINCT goal_unit_id FROM model_usage_calls WHERE company_id=$1 AND run_id=$2",
    )
    .bind(s.company().as_uuid())
    .bind(plain.fence.scope.run.as_uuid())
    .fetch_all(s.pool())
    .await
    .unwrap();
    assert_eq!(plain_goal, vec![None]);
    let finance_day: i64 = sqlx::query_scalar(
        "SELECT spent_micros FROM unit_spend WHERE company_id=$1 AND unit_id=$2 AND period='day'",
    )
    .bind(s.company().as_uuid())
    .bind(finance)
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(finance_day, 30, "the unit's own total is charged as before");
}

/// 03C.2: once the goal's settled spend reaches its `total` cap, a root admission for it — on its
/// desk, and a reply on a thread its call opened in another department — is refused naming the
/// goal; a unit cap is named first when both apply; work serving no goal admits.
#[tokio::test]
async fn goal_budget_total_cap_refuses_admission_and_names_the_goal() {
    let s = CallSetup::new().await;
    price(&s).await;
    let (goal, front) = goal_with_desk(&s, "launch").await;
    let caller = s.claim_from(front, "goal-cap-caller").await;
    s.call(&caller, &to(s.desk, "Plan")).await.unwrap();
    let thread = s.record(caller.fence.scope.run).await.thread_id;
    cap(&s, goal, "total", 100).await.unwrap();
    charge(&s, caller.fence.scope.run.as_uuid(), 99).await;
    created(
        try_admit(
            &s,
            RelatedAssociation::Channel(RelatedChannelId::new(front)),
            "under",
            None,
        )
        .await,
    );
    charge(&s, caller.fence.scope.run.as_uuid(), 1).await;
    let refused = try_admit(
        &s,
        RelatedAssociation::Channel(RelatedChannelId::new(front)),
        "at-cap",
        None,
    )
    .await;
    assert!(goal_exhausted(&refused, goal, "launch"), "{refused:?}");
    let reply = try_admit(&s, on_thread(s.desk, thread), "reply", None).await;
    assert!(goal_exhausted(&reply, goal, "launch"), "{reply:?}");
    created(try_admit(&s, RelatedAssociation::Company, "no-goal", None).await);
    created(
        try_admit(
            &s,
            RelatedAssociation::Channel(RelatedChannelId::new(s.desk)),
            "finance",
            None,
        )
        .await,
    );
    // Both exhausted: the unit on the run's chain is named, as in 07.
    cap(&s, goal, "day", 50).await.unwrap();
    let both = try_admit(
        &s,
        RelatedAssociation::Channel(RelatedChannelId::new(front)),
        "both",
        None,
    )
    .await;
    assert!(
        matches!(&both, Err(AppError::BudgetExhausted(refusal))
            if refusal.unit == goal && refusal.period == "day"
                && refusal.code() == "budget.unit_exhausted"),
        "{both:?}"
    );
}

/// D2 r2 F1: a `total` cap on a goal does not break the 07 chain check for anything in or below
/// the goal while it is not reached (a desk of the goal, a desk of a unit below it, a call into
/// the goal's desk); once reached, the goal's desk refuses with the goal's code.
#[tokio::test]
async fn goal_budget_total_cap_leaves_desks_in_the_goal_admitting() {
    let s = CallSetup::new().await;
    price(&s).await;
    let (goal, front) = goal_with_desk(&s, "launch").await;
    let (_, below_desk) = unit_with_desk(&s, goal, "launch-team").await;
    answering(&s, front).await;
    cap(&s, goal, "total", 1_000).await.unwrap();
    cap(&s, goal, "day", 5_000).await.unwrap();
    let run = created(
        try_admit(
            &s,
            RelatedAssociation::Channel(RelatedChannelId::new(front)),
            "goal-desk",
            None,
        )
        .await,
    );
    created(
        try_admit(
            &s,
            RelatedAssociation::Channel(RelatedChannelId::new(below_desk)),
            "below",
            None,
        )
        .await,
    );
    let caller = s.claim("goal-desk-caller").await;
    assert!(matches!(
        s.call(&caller, &to(front, "Into the goal")).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    charge(&s, run, 1_000).await;
    let refused = try_admit(
        &s,
        RelatedAssociation::Channel(RelatedChannelId::new(front)),
        "goal-desk-late",
        None,
    )
    .await;
    assert!(goal_exhausted(&refused, goal, "launch"), "{refused:?}");
}

/// 03C.3: a goal run admitted before the cap was reached calls another department after it; the
/// child is refused and the caller gets a `denied` closure with the goal's reason, recorded as a
/// refusal and leaving no call row.
#[tokio::test]
async fn goal_budget_refused_call_child_is_a_denied_closure() {
    let s = CallSetup::new().await;
    price(&s).await;
    let (goal, front) = goal_with_desk(&s, "launch").await;
    let spender = s.claim_from(front, "goal-denied-spender").await;
    let caller = s.claim_from(front, "goal-denied-caller").await;
    cap(&s, goal, "total", 100).await.unwrap();
    charge(&s, spender.fence.scope.run.as_uuid(), 100).await;
    let answer = result(s.call(&caller, &to(s.desk, "Over")).await.unwrap());
    assert_eq!(
        answer,
        json!({"closure": "denied", "code": "budget.goal_exhausted",
            "reason": "The goal this work serves has used its spend cap"})
    );
    let refusals: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT channel_id, code FROM workflow_channel_call_refusals WHERE company_id=$1 AND run_id=$2",
    )
    .bind(s.company().as_uuid())
    .bind(caller.fence.scope.run.as_uuid())
    .fetch_all(s.pool())
    .await
    .unwrap();
    assert_eq!(refusals, vec![(s.desk, "budget.goal_exhausted".to_owned())]);
    assert_no_call_rows(&s).await;
}

/// 07 D8 for a goal: an admission waiting on the company lock while a settlement that crosses
/// the goal's cap commits is refused once it proceeds; the settlement never waits on that lock.
#[tokio::test]
async fn goal_competing_admission_waits_and_sees_the_crossing_goal_charge() {
    let s = CallSetup::new().await;
    price(&s).await;
    let (goal, front) = goal_with_desk(&s, "launch").await;
    let spender = s.claim_from(front, "goal-race-spender").await;
    cap(&s, goal, "total", 100).await.unwrap();
    let call = reserve(&s, spender.fence.scope.run.as_uuid(), PRICED, None).await;
    let mut blocker = s.pool().begin().await.unwrap();
    sqlx::query("SELECT 1 FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(s.company().as_uuid())
        .execute(&mut *blocker)
        .await
        .unwrap();
    let association = RelatedAssociation::Channel(RelatedChannelId::new(front));
    let (admitted, ()) = tokio::join!(try_admit(&s, association, "goal-race", None), async {
        wait_until_backends_are_blocked(s.pool(), 1).await;
        tokio::time::timeout(std::time::Duration::from_secs(5), settle(&s, call, 100))
            .await
            .expect("a settlement does not take the company lock");
        blocker.commit().await.unwrap();
    });
    assert!(goal_exhausted(&admitted, goal, "launch"), "{admitted:?}");
}

/// Sixteen settlements for one goal across four working units at once, four queued on each
/// goal row, all commit with exact unit and goal totals and no deadlock.
#[tokio::test]
async fn goal_competing_concurrent_goal_settlements() {
    let s = CallSetup::new().await;
    price(&s).await;
    let (goal, front) = goal_with_desk(&s, "launch").await;
    let run = s
        .claim_from(front, "goal-parallel")
        .await
        .fence
        .scope
        .run
        .as_uuid();
    let mut units = Vec::new();
    for index in 0..4 {
        units.push(
            s.p()
                .test_unit(s.company().as_uuid(), &format!("sibling-{index}"))
                .await,
        );
    }
    let before = goal_rows(&s, goal).await;
    let mut calls = Vec::new();
    for index in 0..16 {
        calls.push(reserve(&s, run, PRICED, Some(units[index % 4])).await);
    }
    let settled = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        futures::future::join_all(calls.iter().map(|call| settle(&s, *call, 10))),
    )
    .await
    .expect("no settlement deadlocks");
    assert_eq!(settled.len(), 16);
    let rows = goal_rows(&s, goal).await;
    for unit in &units {
        assert!(before.iter().all(|row| row.0 != *unit));
        assert!(rows.contains(&(*unit, 40, 0)), "{unit}: {rows:?}");
    }
    assert_eq!(rows, goal_rows_from_calls(&s, goal).await);
}

/// 03D.1: the rollup of a goal with work in three departments equals counts taken from the rows:
/// runs by working unit and state, calls by closure (open, done, and two refusals before launch —
/// the goal's cap and an archived destination — as `denied`), spend per unit against the cap,
/// and the first run and last activity times. A refusal by a run serving no goal is not counted.
#[tokio::test]
async fn goal_rollup_counts_waits_calls_and_spend_match_the_rows() {
    let s = CallSetup::new().await;
    price(&s).await;
    let (goal, front) = goal_with_desk(&s, "launch").await;
    let finance = s.desk_unit().await;
    let root = s.p().test_root_unit(s.company().as_uuid()).await;
    let (ops, ops_desk) = unit_with_desk(&s, root, "ops").await;
    let (_, closed_desk) = unit_with_desk(&s, root, "closed").await;
    answering(&s, ops_desk).await;
    answering(&s, closed_desk).await;
    // Each caller parks on its call before the next is claimed; the refused ones are claimed
    // before the cap is set, as only work already admitted can be refused at its call.
    let open_caller = s.claim_from(front, "rollup-open").await;
    s.call(&open_caller, &to(s.desk, "Open")).await.unwrap();
    // The done caller runs its agent: it calls, parks, and consumes the answer once its child
    // has finished, which records the call's `done` result.
    let done_caller = s.claim_from(front, "rollup-done").await;
    let llm = scripted_llm(vec![
        LlmTurn::tool_call("channel__call", to(ops_desk, "Done")),
        LlmTurn::text("\"answered\""),
    ])
    .await;
    register_scripted_agent_base_url(s.tool.agent, &llm.base_url);
    let runtime = s.tool.runtime();
    let WorkflowHandlerOutcome::Suspended(_) =
        Box::pin(runtime.execute(&kind(), &done_caller)).await
    else {
        panic!("the call parks the agent");
    };
    s.tool.finish_child(done_caller.fence.scope).await;
    let resumed = continued(&s, done_caller.fence.scope).await;
    let WorkflowHandlerOutcome::Completed(Ok(_)) =
        Box::pin(runtime.execute(&kind(), &resumed)).await
    else {
        panic!("the resumed agent consumes the answer and finishes");
    };
    // Its result outlives it; cancelling frees the company's claim capacity for the next callers.
    s.tool
        .cancel(resumed.fence.scope, "rollup-done-cancel")
        .await;
    let open_child = s.record(open_caller.fence.scope.run).await.child_run_id;
    charge(&s, open_caller.fence.scope.run.as_uuid(), 60).await;
    charge(&s, open_child, 40).await;
    let capped_caller = s.claim_from(front, "rollup-capped").await;
    let archived_caller = s.claim_from(front, "rollup-archived").await;
    cap(&s, goal, "total", 100).await.unwrap();
    let capped = result(s.call(&capped_caller, &to(s.desk, "Capped")).await.unwrap());
    assert_eq!(code(&capped), "budget.goal_exhausted");
    // Its refusal outlives it; cancelling frees the company's claim capacity for the next caller.
    s.tool
        .cancel(capped_caller.fence.scope, "rollup-capped-cancel")
        .await;
    let closed_unit: Uuid = sqlx::query_scalar("SELECT department_id FROM channels WHERE id=$1")
        .bind(closed_desk)
        .fetch_one(s.pool())
        .await
        .unwrap();
    archive_raw(&s, closed_unit).await;
    let archived = result(
        s.call(&archived_caller, &to(closed_desk, "Closed"))
            .await
            .unwrap(),
    );
    assert_eq!(archived["closure"], "denied");
    // Not the goal's: a plain run refused by the same archived desk.
    let plain = s.claim("rollup-plain").await;
    let refused = result(s.call(&plain, &to(closed_desk, "Plain")).await.unwrap());
    assert_eq!(refused["closure"], "denied");

    let status = rollup(&s, goal, GOAL_ROLLUP_RUN_LIMIT).await;
    assert_eq!(status.name, "launch");
    assert!(!status.archived && !status.truncated);
    // Runs by working unit and state, counted from the rows.
    let rows: Vec<(Uuid, String, i64)> = sqlx::query_as(
        "SELECT COALESCE(scope.unit_id, $3), run.state, count(*) FROM workflow_runs AS run \
           LEFT JOIN workflow_run_scopes AS scope ON scope.company_id=run.company_id \
            AND scope.run_id=run.id AND scope.acting_principal_id IS NULL AND scope.access='write' \
          WHERE run.company_id=$1 AND run.goal_unit_id=$2 GROUP BY 1, 2",
    )
    .bind(s.company().as_uuid())
    .bind(goal)
    .bind(root)
    .fetch_all(s.pool())
    .await
    .unwrap();
    let mut expected: Vec<(Uuid, String, i64)> = Vec::new();
    for unit in &status.units {
        for (state, count) in [
            ("queued", unit.queued),
            ("running", unit.running),
            ("waiting", unit.waiting),
            ("succeeded", unit.succeeded),
            ("failed", unit.failed),
            ("cancelled", unit.cancelled),
        ] {
            if count > 0 {
                expected.push((unit.unit, state.to_owned(), count));
            }
        }
    }
    let mut rows = rows;
    rows.sort();
    expected.sort();
    assert_eq!(expected, rows);
    assert_eq!(
        status.runs_counted,
        rows.iter().map(|row| row.2).sum::<i64>()
    );
    let worked: std::collections::BTreeSet<Uuid> = rows.iter().map(|row| row.0).collect();
    assert_eq!(
        worked,
        [goal, finance, ops].into_iter().collect(),
        "three departments"
    );
    let named = |unit: Uuid| {
        status
            .units
            .iter()
            .find(|row| row.unit == unit)
            .unwrap()
            .clone()
    };
    assert_eq!(named(goal).name.as_deref(), Some("launch"));
    assert_eq!(named(finance).name.as_deref(), Some("finance-unit"));
    let goal_runs = named(goal);
    assert_eq!(
        goal_runs.queued
            + goal_runs.running
            + goal_runs.waiting
            + goal_runs.succeeded
            + goal_runs.failed
            + goal_runs.cancelled,
        4,
        "the four callers"
    );
    // Calls by closure.
    assert_eq!(status.calls.open, 1);
    assert_eq!(status.calls.done, 1);
    assert_eq!(status.calls.denied, 2, "the goal cap and the archived desk");
    assert_eq!(
        status.calls.handed_off + status.calls.no_reply + status.calls.error,
        0
    );
    // Spend against the cap, per working unit, as the settled calls give it.
    assert_eq!(status.spend.cap_micros, Some(100));
    let spend: Vec<(Uuid, i64, i64)> = status
        .units
        .iter()
        .filter(|row| row.spent_micros > 0 || row.unpriced_tokens > 0)
        .map(|row| (row.unit, row.spent_micros, row.unpriced_tokens))
        .collect();
    let mut spend = spend;
    spend.sort();
    assert_eq!(spend, goal_rows_from_calls(&s, goal).await);
    assert_eq!(named(goal).spent_micros, 60);
    assert_eq!(named(finance).spent_micros, 40);
    assert_eq!(status.spend.spent_micros, 100);
    // Times.
    let (first, last_created, last_finished): (
        chrono::DateTime<chrono::Utc>,
        chrono::DateTime<chrono::Utc>,
        Option<chrono::DateTime<chrono::Utc>>,
    ) = sqlx::query_as(
        "SELECT min(created_at), max(created_at), max(finished_at) FROM workflow_runs \
          WHERE company_id=$1 AND goal_unit_id=$2",
    )
    .bind(s.company().as_uuid())
    .bind(goal)
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(status.first_run_at, Some(first));
    assert_eq!(status.last_finished_at, last_finished);
    assert_eq!(
        status.last_activity_at,
        Some(last_finished.map_or(last_created, |finished| finished.max(last_created)))
    );
    assert!(status.waiting.is_empty() && status.waiting_total == 0);
    assert_eq!(status.escalations_open, 0);
    assert_eq!(status.waiting_on_people_seconds, 0);
}

/// 03D.1 limit: with more runs than the limit, the rollup says so and counts the newest; the
/// first run is still the goal's first.
#[tokio::test]
async fn goal_rollup_reports_truncation() {
    let s = CallSetup::new().await;
    let (goal, front) = goal_with_desk(&s, "launch").await;
    let association = || RelatedAssociation::Channel(RelatedChannelId::new(front));
    let oldest = created(try_admit(&s, association(), "trunc-1", None).await);
    let middle = s
        .claim_from(front, "trunc-2")
        .await
        .fence
        .scope
        .run
        .as_uuid();
    let newest = s
        .claim_from(front, "trunc-3")
        .await
        .fence
        .scope
        .run
        .as_uuid();
    let status = rollup(&s, goal, 2).await;
    assert!(status.truncated);
    assert_eq!(status.runs_counted, 2);
    let unit = &status.units[0];
    assert_eq!(
        (unit.queued, unit.running),
        (0, 2),
        "the two newest are running"
    );
    let first: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT created_at FROM workflow_runs WHERE id=$1")
            .bind(oldest)
            .fetch_one(s.pool())
            .await
            .unwrap();
    assert_eq!(status.first_run_at, Some(first));
    let whole = rollup(&s, goal, 3).await;
    assert!(!whole.truncated);
    assert_eq!((whole.units[0].queued, whole.units[0].running), (1, 2));
    assert_ne!(middle, newest);
}

async fn status(
    s: &CallSetup,
    origin: crate::application::agent::AgentToolOrigin,
    fence: crate::application::workflow::lease::WorkflowFence,
    arguments: serde_json::Value,
) -> serde_json::Value {
    result(
        s.p()
            .goal_status(fence, GoalStatusToolCall { origin, arguments })
            .await
            .unwrap(),
    )
}

/// 03D.4: `goal.status` gives a lead's or sponsor's agent the same JSON as the rollup; it takes
/// no arguments; a run serving no goal, a goal that became standing, and an agent holding no role
/// on the goal each get a named refusal.
#[tokio::test]
async fn goal_rollup_status_tool_returns_the_rollup_and_refuses_without_a_goal() {
    let s = CallSetup::new().await;
    let (goal, front) = goal_with_desk(&s, "launch").await;
    let caller = s.claim_from(front, "status-caller").await;
    let origin = s
        .tool
        .channel_call(&s.arguments("Origin"), &caller)
        .await
        .origin;
    let refused = status(&s, origin, caller.fence, json!({})).await;
    assert_eq!(code(&refused), GOAL_NOT_AUTHORIZED, "{refused}");
    let agent_principal: Uuid = sqlx::query_scalar(
        "SELECT id FROM principals WHERE company_id=$1 AND agent_id=$2 AND kind='agent'",
    )
    .bind(s.company().as_uuid())
    .bind(s.tool.agent)
    .fetch_one(s.pool())
    .await
    .unwrap();
    for role in ["sponsor", "lead"] {
        sqlx::query(
            "DELETE FROM department_roles WHERE company_id=$1 AND department_id=$2 AND principal_id=$3",
        )
        .bind(s.company().as_uuid())
        .bind(goal)
        .bind(agent_principal)
        .execute(s.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO department_roles (company_id, department_id, role, principal_id) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(s.company().as_uuid())
        .bind(goal)
        .bind(role)
        .bind(agent_principal)
        .execute(s.pool())
        .await
        .unwrap();
        let answer = status(&s, origin, caller.fence, json!({})).await;
        let expected = serde_json::to_value(rollup(&s, goal, GOAL_ROLLUP_RUN_LIMIT).await).unwrap();
        assert_eq!(answer, json!({"status": expected}), "{role}");
    }
    let invalid = status(&s, origin, caller.fence, json!({"goal": goal})).await;
    assert_eq!(code(&invalid), "invalid_input");
    let plain = s.claim("status-plain").await;
    let none = status(&s, origin, plain.fence, json!({})).await;
    assert_eq!(code(&none), GOAL_NONE);
    assert_eq!(none["error"]["message"], "This run serves no goal");
    sqlx::query("UPDATE departments SET kind='standing' WHERE id=$1")
        .bind(goal)
        .execute(s.pool())
        .await
        .unwrap();
    let standing = status(&s, origin, caller.fence, json!({})).await;
    assert_eq!(code(&standing), GOAL_NONE);
    assert_eq!(
        standing["error"]["message"],
        "This run's goal is no longer a goal"
    );
}

/// Plan08 p04 (the stand-in p03 left): an agent holding `goal.status` and the goal's lead role
/// calls it from a real agent step on the goal's desk; the model is offered `goal__status`, and
/// the tool result it reads next is the goal's status.
#[tokio::test]
async fn goal_rollup_status_tool_through_the_agent_runtime() {
    use crate::entities::agent_capability::AgentBuiltinTool;
    let s = Box::pin(CallSetup::with_tools(
        destination(None),
        &[AgentBuiltinTool::ChannelCall, AgentBuiltinTool::GoalStatus],
    ))
    .await;
    let (goal, front) = goal_with_desk(&s, "launch").await;
    sqlx::query(
        "INSERT INTO department_roles (company_id, department_id, role, principal_id) \
         SELECT $1, $2, 'lead', id FROM principals \
          WHERE company_id = $1 AND agent_id = $3 AND kind = 'agent'",
    )
    .bind(s.company().as_uuid())
    .bind(goal)
    .bind(s.tool.agent)
    .execute(s.pool())
    .await
    .unwrap();
    let caller = s.claim_from(front, "status-runtime").await;
    let mut llm = scripted_llm(vec![
        LlmTurn::tool_call("goal__status", json!({})),
        LlmTurn::text("\"read\""),
    ])
    .await;
    register_scripted_agent_base_url(s.tool.agent, &llm.base_url);
    let runtime = s.tool.runtime();
    let WorkflowHandlerOutcome::Completed(Ok(_)) =
        Box::pin(runtime.execute(&kind(), &caller)).await
    else {
        panic!("the agent reads its goal's status and finishes");
    };
    let first = llm.requests.recv().await.unwrap();
    let offered: Vec<&str> = first["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["function"]["name"].as_str())
        .collect();
    assert!(offered.contains(&"goal__status"), "{offered:?}");
    let second = llm.requests.recv().await.unwrap();
    let answer = second["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["role"] == "tool")
        .expect("the tool result");
    let content: serde_json::Value =
        serde_json::from_str(answer["content"].as_str().unwrap()).unwrap();
    let status = &content["status"];
    assert_eq!(status["goal"], goal.to_string(), "{content}");
    assert_eq!(status["name"], "launch");
    assert_eq!(status["runs_counted"], 1, "the calling run itself");
    assert_eq!(status["units"][0]["running"], 1);
}
