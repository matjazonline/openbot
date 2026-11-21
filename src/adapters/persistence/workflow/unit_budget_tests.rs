//! Plan07 02B: usage attribution to the run's write unit, the running totals the settlement
//! trigger keeps, and the admission cap check, with the competing-claimant cases of G1 r3–r5.
use super::*;
use crate::adapters::persistence::test_support::wait_until_backends_are_blocked;
use crate::services::model_usage::{UsageGrouping, UsageQuery, UsageReport, UsageScope};
use crate::use_cases::company::CompanyPersistence;

struct Budgets {
    f: AdmissionFixture,
    company: Uuid,
    owner: Uuid,
    root: Uuid,
    billing: Uuid,
    support: Uuid,
    agent: Uuid,
}

impl Budgets {
    async fn new() -> Self {
        let f = AdmissionFixture::new().await;
        let company = f.binding.target.company.as_uuid();
        let owner = f.binding.target.actor.user_id();
        let p = f.persistence();
        let root = p.test_root_unit(company).await;
        let billing = p.test_unit(company, "billing").await;
        let support = p.test_unit(company, "support").await;
        // One micro-USD per input token, two per output token.
        sqlx::query(
            "INSERT INTO model_prices(company_id, provider, model, input_micro_usd_per_million, \
             output_micro_usd_per_million) VALUES ($1, 'openai', 'fixture', 1000000, 2000000)",
        )
        .bind(company)
        .execute(p.pool())
        .await
        .unwrap();
        let agent = crate::use_cases::agent::AgentPersistence::create(
            p,
            company,
            crate::use_cases::agent::AgentWrite {
                name: "Budget agent".into(),
                slug: "budget-agent".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        Self {
            f,
            agent,
            company,
            owner,
            root,
            billing,
            support,
        }
    }
    fn p(&self) -> &PostgresPersistence {
        self.f.persistence()
    }
    async fn desk(&self, unit: Uuid) -> RelatedAssociation {
        let channel = crate::adapters::persistence::test_support::create_channel(
            self.p(),
            self.company,
            crate::application::ports::channel::ChannelWrite {
                name: "Unit desk".into(),
                slug: format!("desk-{}", Uuid::new_v4().simple()),
                department_id: Some(crate::entities::department::DepartmentId::new(unit)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        RelatedAssociation::Channel(crate::application::workflow::RelatedChannelId::new(
            channel.id,
        ))
    }
    /// Admit a run on `association`; `Ok(run)` or the admission's refusal.
    async fn admit(&self, association: RelatedAssociation) -> AppResult<PreparedAdmission> {
        let mut request = self.f.request(&Uuid::new_v4().to_string(), self.f.manual());
        request.association = association;
        let command = self.f.prepare(request).await;
        self.p().admit(&command).await.map(|_| command)
    }
    /// A reserved model call of `run` (`unit` overrides the attribution), returning its id.
    async fn call(
        &self,
        command: &PreparedAdmission,
        model: &str,
        unit: Option<Uuid>,
        requested_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Uuid {
        self.call_on(
            command.proposed_run_id().as_uuid(),
            command.first_execution_id().as_uuid(),
            model,
            unit,
            requested_at,
        )
        .await
    }
    /// A reserved model call of `run`'s `execution`.
    async fn call_on(
        &self,
        run: Uuid,
        execution: Uuid,
        model: &str,
        unit: Option<Uuid>,
        requested_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Uuid {
        let mut row: Value = sqlx::query_scalar("SELECT jsonb_build_object('company_id',run.company_id,'request_id',gen_random_uuid(),
            'task_id',task.id,'agent_id',$4::uuid,'workflow_id',run.workflow_id,'version_id',run.version_id,'run_id',run.id,
            'execution_id',task.workflow_execution_id,'root_run_id',budget.root_run_id,'provider','openai','model',$3::text,
            'reserved_input',100,'reserved_output',50,'requested_at',statement_timestamp())
            FROM workflow_runs AS run JOIN background_tasks AS task ON task.company_id=run.company_id AND task.workflow_execution_id=$2
            JOIN workflow_run_budgets AS budget ON budget.company_id=run.company_id AND budget.run_id=run.id WHERE run.id=$1")
            .bind(run).bind(execution).bind(model).bind(self.agent)
            .fetch_one(self.p().pool()).await.unwrap();
        if let Some(unit) = unit {
            row["unit_id"] = json!(unit);
        }
        if let Some(at) = requested_at {
            row["requested_at"] = json!(at);
        }
        sqlx::query("INSERT INTO model_usage_calls SELECT (jsonb_populate_record(NULL::model_usage_calls,$1)).*")
            .bind(&row).execute(self.p().pool()).await.unwrap();
        serde_json::from_value(row["request_id"].clone()).unwrap()
    }
    fn settle_sql() -> &'static str {
        "UPDATE model_usage_calls SET input_tokens=$3, output_tokens=$4, usage_source='reported', \
         settled_at=statement_timestamp() WHERE company_id=$1 AND request_id=$2"
    }
    async fn settle(&self, request: Uuid, input: i64, output: i64) {
        sqlx::query(Self::settle_sql())
            .bind(self.company)
            .bind(request)
            .bind(input)
            .bind(output)
            .execute(self.p().pool())
            .await
            .unwrap();
    }
    /// (spent micros, unpriced tokens) of `unit` for the period containing `at` (now by default).
    async fn spent(&self, unit: Uuid, period: &str) -> (i64, i64) {
        self.spent_at(unit, period, chrono::Utc::now()).await
    }
    async fn spent_at(
        &self,
        unit: Uuid,
        period: &str,
        at: chrono::DateTime<chrono::Utc>,
    ) -> (i64, i64) {
        sqlx::query_as(
            "SELECT spent_micros, unpriced_tokens FROM unit_spend WHERE company_id=$1 AND unit_id=$2 \
             AND period=$3 AND period_start = date_trunc($3, $4::timestamptz AT TIME ZONE 'UTC') \
             AT TIME ZONE 'UTC'",
        )
        .bind(self.company)
        .bind(unit)
        .bind(period)
        .bind(at)
        .fetch_optional(self.p().pool())
        .await
        .unwrap()
        .unwrap_or((0, 0))
    }
    async fn unit_of(&self, request: Uuid) -> Uuid {
        sqlx::query_scalar(
            "SELECT unit_id FROM model_usage_calls WHERE company_id=$1 AND request_id=$2",
        )
        .bind(self.company)
        .bind(request)
        .fetch_one(self.p().pool())
        .await
        .unwrap()
    }
    async fn cap(&self, unit: Uuid, period: &str, cap: i64) {
        CompanyPersistence::set_unit_budget(self.p(), self.owner, self.company, unit, period, cap)
            .await
            .unwrap();
    }
}

fn exhausted(result: &AppResult<PreparedAdmission>, unit: Uuid, period: &str) -> bool {
    matches!(result, Err(AppError::BudgetExhausted(refusal))
        if refusal.unit == unit && refusal.period == period
            && refusal.code() == "budget.unit_exhausted")
}

/// Plan07 02B.3 with D5/D7: a call is attributed to its run's write unit (the root for a
/// company-wide run); settling it charges its priced cost, rounded up, to that unit and every
/// ancestor for the UTC day and month containing its request; unpriced tokens are counted apart;
/// an unsettled call is not counted; a call requested before midnight UTC is charged to that day.
#[tokio::test]
async fn unit_budget_charge_attributes_and_totals_settled_calls() {
    let b = Budgets::new().await;
    let billing_run = b.admit(b.desk(b.billing).await).await.unwrap();
    let rooted_run = b.admit(RelatedAssociation::Company).await.unwrap();
    let billed = b.call(&billing_run, "fixture", None, None).await;
    let unit: Uuid = sqlx::query_scalar(
        "SELECT unit_id FROM model_usage_calls WHERE company_id=$1 AND request_id=$2",
    )
    .bind(b.company)
    .bind(billed)
    .fetch_one(b.p().pool())
    .await
    .unwrap();
    assert_eq!(unit, b.billing, "the desk's unit");
    // 100 input + 50 output tokens = 100 + 100 micro-USD; 7 + 0 tokens round up to 7.
    b.settle(billed, 100, 50).await;
    let small = b.call(&billing_run, "fixture", None, None).await;
    b.settle(small, 7, 0).await;
    for unit in [b.billing, b.root] {
        for period in ["day", "month"] {
            assert_eq!(b.spent(unit, period).await, (207, 0), "{unit} {period}");
        }
    }
    assert_eq!(b.spent(b.support, "day").await, (0, 0));
    let unpriced = b.call(&rooted_run, "unpriced-model", None, None).await;
    assert_eq!(
        b.unit_of(unpriced).await,
        b.root,
        "a company-wide run is the root's"
    );
    b.settle(unpriced, 10, 5).await;
    assert_eq!(b.spent(b.root, "day").await, (207, 15));
    let pending = b.call(&rooted_run, "fixture", None, None).await;
    assert_eq!(
        b.spent(b.root, "day").await,
        (207, 15),
        "unsettled: {pending}"
    );
    let yesterday = chrono::Utc::now().date_naive().pred_opt().unwrap();
    let late = yesterday.and_hms_opt(23, 59, 59).unwrap().and_utc();
    let overnight = b.call(&rooted_run, "fixture", None, Some(late)).await;
    b.settle(overnight, 1, 0).await;
    assert_eq!(b.spent_at(b.root, "day", late).await, (1, 0));
    assert_eq!(b.spent(b.root, "day").await, (207, 15));
}

/// Plan07 02B.2 with D7/D8: admission refuses a run whose write unit, or an ancestor, has spent
/// a cap, naming the nearest exhausted unit (day before month); a sibling is unaffected; unpriced
/// usage never exhausts a cap.
#[tokio::test]
async fn unit_budget_admission_refuses_at_the_nearest_exhausted_unit() {
    let b = Budgets::new().await;
    let billing = b.desk(b.billing).await;
    let support = b.desk(b.support).await;
    let run = b.admit(billing).await.unwrap();
    let call = b.call(&run, "fixture", None, None).await;
    b.settle(call, 100, 50).await;
    b.cap(b.billing, "day", 200).await;
    assert!(exhausted(&b.admit(billing).await, b.billing, "day"));
    b.admit(support).await.unwrap();
    b.admit(RelatedAssociation::Company).await.unwrap();
    // A root month cap stops every desk; a run on Billing still names Billing, the nearer.
    b.cap(b.root, "day", 1_000_000).await;
    b.cap(b.root, "month", 150).await;
    assert!(exhausted(
        &b.admit(RelatedAssociation::Company).await,
        b.root,
        "month"
    ));
    assert!(exhausted(&b.admit(support).await, b.root, "month"));
    assert!(exhausted(&b.admit(billing).await, b.billing, "day"));
    // Unpriced usage counts zero (decision h).
    let fresh = Budgets::new().await;
    let run = fresh.admit(RelatedAssociation::Company).await.unwrap();
    let call = fresh.call(&run, "unpriced-model", None, None).await;
    fresh.settle(call, 1_000, 1_000).await;
    fresh.cap(fresh.root, "day", 1).await;
    fresh.admit(RelatedAssociation::Company).await.unwrap();
}

/// Plan07 02B.1, decision c: a unit's cap is set by whoever manages that unit — its lead or
/// anyone above — never by a child's lead or a member; periods are day or month, caps positive.
#[tokio::test]
async fn unit_budget_authority_follows_the_units_manager() {
    let b = Budgets::new().await;
    let p = b.p();
    let field = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO departments (id, company_id, parent_id, name, slug, created_by) \
         VALUES (gen_random_uuid(), $1, $2, 'field', 'field', \
         '{\"actor_type\":\"system\",\"actor_id\":null,\"actor_name\":\"Fixture\"}') RETURNING id",
    )
    .bind(b.company)
    .bind(b.billing)
    .fetch_one(p.pool())
    .await
    .unwrap();
    let billing_lead = p.test_unit_lead(b.company, b.billing).await;
    let field_lead = p.test_unit_lead(b.company, field).await;
    let set = |user: Uuid, unit: Uuid, period: &'static str, cap: i64| {
        CompanyPersistence::set_unit_budget(p, user, b.company, unit, period, cap)
    };
    set(billing_lead, b.billing, "day", 500).await.unwrap();
    set(billing_lead, field, "month", 900).await.unwrap();
    assert!(matches!(
        set(field_lead, b.billing, "day", 1).await,
        Err(AppError::NotFound(_))
    ));
    assert!(matches!(
        set(billing_lead, b.support, "day", 1).await,
        Err(AppError::NotFound(_))
    ));
    assert!(matches!(
        set(b.owner, b.billing, "week", 1).await,
        Err(AppError::BadRequest(_))
    ));
    assert!(matches!(
        set(b.owner, b.billing, "day", 0).await,
        Err(AppError::BadRequest(_))
    ));
    CompanyPersistence::clear_unit_budget(p, billing_lead, b.company, field, "month")
        .await
        .unwrap();
    let caps: Vec<(Uuid, String, i64)> = sqlx::query_as(
        "SELECT unit_id, period, cap_micros FROM unit_budgets WHERE company_id=$1 ORDER BY period",
    )
    .bind(b.company)
    .fetch_all(p.pool())
    .await
    .unwrap();
    assert_eq!(caps, vec![(b.billing, "day".to_owned(), 500)]);
}

/// Plan07 G1 r4/r5 (F11, F14, F16): deleting a charged department removes only its own totals
/// (ancestors keep the call); a call settling after its unit was deleted charges the root; a
/// department row held by an unrelated write does not delay a settlement.
#[tokio::test]
async fn unit_budget_competing_delete_and_department_writes() {
    let b = Budgets::new().await;
    let p = b.p();
    let run = b.admit(RelatedAssociation::Company).await.unwrap();
    let call = b.call(&run, "fixture", Some(b.billing), None).await;
    b.settle(call, 10, 0).await;
    sqlx::query("DELETE FROM departments WHERE company_id=$1 AND id=$2")
        .bind(b.company)
        .bind(b.billing)
        .execute(p.pool())
        .await
        .unwrap();
    assert_eq!(b.spent(b.billing, "day").await, (0, 0), "its rows are gone");
    assert_eq!(b.spent(b.root, "day").await, (10, 0), "the root keeps it");
    let orphan = b.call(&run, "fixture", Some(b.billing), None).await;
    b.settle(orphan, 5, 0).await;
    assert_eq!(
        b.spent(b.root, "day").await,
        (15, 0),
        "a deleted unit charges the root"
    );
    assert_eq!(b.spent(b.billing, "day").await, (0, 0));
    // A member change holding Support's row FOR UPDATE does not block a Support charge.
    let mut held = p.pool().begin().await.unwrap();
    sqlx::query("SELECT 1 FROM departments WHERE company_id=$1 AND id IN ($2, $3) FOR UPDATE")
        .bind(b.company)
        .bind(b.support)
        .bind(b.root)
        .execute(&mut *held)
        .await
        .unwrap();
    let supported = b.call(&run, "fixture", Some(b.support), None).await;
    tokio::time::timeout(std::time::Duration::from_secs(5), b.settle(supported, 3, 0))
        .await
        .expect("a settlement does not wait on department rows");
    held.rollback().await.unwrap();
    assert_eq!(b.spent(b.support, "day").await, (3, 0));
}

/// Plan07 D8 (decision k): an admission waiting on the company lock while a settlement that
/// crosses the cap commits is refused once it proceeds; settlements never wait on that lock.
#[tokio::test]
async fn unit_budget_competing_admission_waits_and_sees_the_crossing_charge() {
    let b = Budgets::new().await;
    let p = b.p();
    let run = b.admit(RelatedAssociation::Company).await.unwrap();
    b.cap(b.root, "day", 100).await;
    let call = b.call(&run, "fixture", None, None).await;
    let mut blocker = p.pool().begin().await.unwrap();
    sqlx::query("SELECT 1 FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(b.company)
        .execute(&mut *blocker)
        .await
        .unwrap();
    let (admitted, ()) = tokio::join!(b.admit(RelatedAssociation::Company), async {
        wait_until_backends_are_blocked(p.pool(), 1).await;
        tokio::time::timeout(std::time::Duration::from_secs(5), b.settle(call, 100, 0))
            .await
            .expect("a settlement does not take the company lock");
        blocker.commit().await.unwrap();
    });
    assert!(exhausted(&admitted, b.root, "day"), "{:?}", admitted.err());
}

/// Plan07 G1 r3/r5 (F12, F16): sixteen settlements across sibling units at once all commit with
/// exact totals; a charge holding a unit's rows, a delete of that unit and a second charge
/// queued behind it all complete without a deadlock.
#[tokio::test]
async fn unit_budget_competing_concurrent_settlements_and_lock_order() {
    let b = Budgets::new().await;
    let p = b.p();
    let run = b.admit(RelatedAssociation::Company).await.unwrap();
    let mut units = Vec::new();
    for index in 0..4 {
        units.push(p.test_unit(b.company, &format!("sibling-{index}")).await);
    }
    let mut calls = Vec::new();
    for index in 0..16 {
        calls.push(b.call(&run, "fixture", Some(units[index % 4]), None).await);
    }
    let settled = futures::future::join_all(calls.iter().map(|call| b.settle(*call, 10, 0))).await;
    assert_eq!(settled.len(), 16);
    assert_eq!(b.spent(b.root, "day").await, (160, 0));
    for unit in &units {
        assert_eq!(b.spent(*unit, "day").await, (40, 0));
    }
    assert_eq!(b.spent(b.root, "day").await, (160, 0));
}

/// Plan07 G1 r5 (F16): the delete cleanup takes a unit's totals in the charge's order even when
/// the month row sits before the day row in the heap and the deleting session scans it
/// sequentially: a charge holding the month row, a delete queued on it and a second charge
/// holding the day row all finish, with no deadlock (an unordered delete would deadlock here).
#[tokio::test]
async fn unit_budget_competing_delete_takes_totals_in_the_charges_order() {
    let b = Budgets::new().await;
    let p = b.p();
    let run = b.admit(RelatedAssociation::Company).await.unwrap();
    let target = p.test_unit(b.company, "target").await;
    // The month row first in the heap, then the day row.
    for period in ["month", "day"] {
        sqlx::query(
            "INSERT INTO unit_spend (company_id, unit_id, period, period_start, spent_micros) \
             VALUES ($1, $2, $3, date_trunc($3, clock_timestamp() AT TIME ZONE 'UTC') \
             AT TIME ZONE 'UTC', 1)",
        )
        .bind(b.company)
        .bind(target)
        .bind(period)
        .execute(p.pool())
        .await
        .unwrap();
    }
    let charge = b.call(&run, "fixture", Some(target), None).await;
    // A charge part-way through: it holds the month row.
    let mut holding = p.pool().begin().await.unwrap();
    sqlx::query(
        "SELECT 1 FROM unit_spend WHERE company_id=$1 AND unit_id=$2 AND period='month' FOR UPDATE",
    )
    .bind(b.company)
    .bind(target)
    .execute(&mut *holding)
    .await
    .unwrap();
    let pool = p.pool().clone();
    let company = b.company;
    let deleting = tokio::spawn(async move {
        let mut tx = pool.begin().await?;
        sqlx::query("SET LOCAL enable_indexscan = off")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SET LOCAL enable_bitmapscan = off")
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM departments WHERE company_id=$1 AND id=$2")
            .bind(company)
            .bind(target)
            .execute(&mut *tx)
            .await?;
        tx.commit().await
    });
    wait_until_backends_are_blocked(p.pool(), 1).await;
    let pool = p.pool().clone();
    let charging = tokio::spawn(async move {
        let mut tx = pool.begin().await?;
        sqlx::query("SET CONSTRAINTS model_usage_charges_unit_spend IMMEDIATE")
            .execute(&mut *tx)
            .await?;
        sqlx::query(Budgets::settle_sql())
            .bind(company)
            .bind(charge)
            .bind(1_i64)
            .bind(0_i64)
            .execute(&mut *tx)
            .await?;
        tx.commit().await
    });
    wait_until_backends_are_blocked(p.pool(), 2).await;
    holding.commit().await.unwrap();
    let timeout = std::time::Duration::from_secs(10);
    tokio::time::timeout(timeout, deleting)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    tokio::time::timeout(timeout, charging)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

/// Plan07 decision j (B2-F1): a re-parented department's later charges follow its new chain,
/// while what its old parent counted stays; a child run's calls are attributed to the write unit
/// it inherited from its parent.
#[tokio::test]
async fn unit_budget_charge_follows_a_reparented_unit_and_a_childs_inherited_scope() {
    let b = Budgets::new().await;
    let p = b.p();
    let run = b.admit(RelatedAssociation::Company).await.unwrap();
    let field = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO departments (id, company_id, parent_id, name, slug, created_by) \
         VALUES (gen_random_uuid(), $1, $2, 'field', 'field', \
         '{\"actor_type\":\"system\",\"actor_id\":null,\"actor_name\":\"Fixture\"}') RETURNING id",
    )
    .bind(b.company)
    .bind(b.billing)
    .fetch_one(p.pool())
    .await
    .unwrap();
    let before = b.call(&run, "fixture", Some(field), None).await;
    b.settle(before, 10, 0).await;
    sqlx::query("UPDATE departments SET parent_id=$3 WHERE company_id=$1 AND id=$2")
        .bind(b.company)
        .bind(field)
        .bind(b.support)
        .execute(p.pool())
        .await
        .unwrap();
    let after = b.call(&run, "fixture", Some(field), None).await;
    b.settle(after, 3, 0).await;
    assert_eq!(
        b.spent(b.billing, "day").await,
        (10, 0),
        "the old parent keeps its count"
    );
    assert_eq!(
        b.spent(b.support, "day").await,
        (3, 0),
        "the new parent counts from now"
    );
    assert_eq!(b.spent(field, "day").await, (13, 0));
    assert_eq!(b.spent(b.root, "day").await, (13, 0));
    // A child of a Billing-desk run inherits its scope rows, so its calls are Billing's.
    let leaf: Value = serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
    let caller = AdmissionFixture::calling(leaf).await;
    let company = caller.binding.target.company.as_uuid();
    let billing = caller.persistence().test_unit(company, "billing").await;
    let channel = crate::adapters::persistence::test_support::create_channel(
        caller.persistence(),
        company,
        crate::application::ports::channel::ChannelWrite {
            name: "Billing desk".into(),
            slug: "billing-desk".into(),
            department_id: Some(crate::entities::department::DepartmentId::new(billing)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let mut request = caller.request("child-attribution", caller.manual());
    request.association = RelatedAssociation::Channel(
        crate::application::workflow::RelatedChannelId::new(channel.id),
    );
    let command = caller.prepare(request).await;
    caller.persistence().admit(&command).await.unwrap();
    let root = super::child_support::entry(&caller, command.proposed_run_id()).await;
    let child = caller.spawn(root).await;
    let agent = crate::use_cases::agent::AgentPersistence::create(
        caller.persistence(),
        company,
        crate::use_cases::agent::AgentWrite {
            name: "Child agent".into(),
            slug: "child-agent".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let row: Value = sqlx::query_scalar("SELECT jsonb_build_object('company_id',run.company_id,'request_id',gen_random_uuid(),
        'task_id',task.id,'agent_id',$3::uuid,'workflow_id',run.workflow_id,'version_id',run.version_id,'run_id',run.id,
        'execution_id',task.workflow_execution_id,'root_run_id',budget.root_run_id,'provider','openai','model','fixture',
        'reserved_input',1,'reserved_output',1,'requested_at',statement_timestamp())
        FROM workflow_runs AS run JOIN background_tasks AS task ON task.company_id=run.company_id AND task.workflow_execution_id=$2
        JOIN workflow_run_budgets AS budget ON budget.company_id=run.company_id AND budget.run_id=run.id WHERE run.id=$1")
        .bind(child.run.as_uuid()).bind(child.execution.as_uuid()).bind(agent)
        .fetch_one(caller.persistence().pool()).await.unwrap();
    let unit: Uuid = sqlx::query_scalar(
        "INSERT INTO model_usage_calls SELECT (jsonb_populate_record(NULL::model_usage_calls,$1)).* \
         RETURNING unit_id",
    )
    .bind(&row)
    .fetch_one(caller.persistence().pool())
    .await
    .unwrap();
    assert_eq!(
        unit, billing,
        "the child inherits the Billing desk's write unit"
    );
}

fn usage_query(
    b: &Budgets,
    actor: Uuid,
    grouping: UsageGrouping,
    unit: Option<Uuid>,
) -> UsageQuery {
    let end = chrono::Utc::now() + chrono::Duration::minutes(1);
    UsageQuery {
        scope: UsageScope {
            company: b.company,
            actor,
        },
        start: end - chrono::Duration::days(2),
        end,
        grouping,
        agent: None,
        workflow: None,
        unit,
    }
}

/// Plan07 02C (D9 r2): the usage report groups calls by the department they were attributed to;
/// a unit's lead sees only their departments' calls and caps, a root manager every one (a deleted
/// department's calls too); counted spend, caps and remaining equal the running totals; unpriced
/// tokens are flagged; the drill-down groups one department's calls by desk; price writes stay
/// with root managers.
#[tokio::test]
async fn unit_budget_usage_page_scopes_numbers_drill_down_and_prices() {
    use crate::services::model_usage::{ModelUsageStore, PriceWrite};
    let b = Budgets::new().await;
    let p = b.p();
    let billing_desk = b.desk(b.billing).await;
    let billing_run = b.admit(billing_desk).await.unwrap();
    let support_run = b.admit(b.desk(b.support).await).await.unwrap();
    let billed = b.call(&billing_run, "fixture", None, None).await;
    b.settle(billed, 100, 50).await;
    let unpriced = b.call(&billing_run, "unpriced-model", None, None).await;
    b.settle(unpriced, 4, 1).await;
    let supported = b.call(&support_run, "fixture", None, None).await;
    b.settle(supported, 7, 0).await;
    b.cap(b.billing, "month", 1_000).await;
    // A call of Billing's sub-department is Billing's lead's to see too.
    let field = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO departments (id, company_id, parent_id, name, slug, created_by) \
         VALUES (gen_random_uuid(), $1, $2, 'field', 'field', \
         '{\"actor_type\":\"system\",\"actor_id\":null,\"actor_name\":\"Fixture\"}') RETURNING id",
    )
    .bind(b.company)
    .bind(b.billing)
    .fetch_one(p.pool())
    .await
    .unwrap();
    let fielded = b.call(&billing_run, "fixture", Some(field), None).await;
    b.settle(fielded, 1, 0).await;
    let gone = p.test_unit(b.company, "gone").await;
    let orphan = b.call(&support_run, "fixture", Some(gone), None).await;
    b.settle(orphan, 1, 0).await;
    sqlx::query("DELETE FROM departments WHERE company_id=$1 AND id=$2")
        .bind(b.company)
        .bind(gone)
        .execute(p.pool())
        .await
        .unwrap();
    let lead = p.test_unit_lead(b.company, b.billing).await;
    let departments = |report: &UsageReport| {
        let mut names: Vec<String> = report
            .groups
            .iter()
            .map(|group| group.department.as_ref().unwrap().name.clone())
            .collect();
        names.sort();
        names
    };
    let led = p
        .usage_report(usage_query(&b, lead, UsageGrouping::Department, None))
        .await
        .unwrap();
    assert_eq!(departments(&led), ["billing", "field"]);
    assert!(!led.prices_editable);
    type Row = (String, String, Option<String>, String, Option<String>, u64);
    let budgets: Vec<Row> = led
        .budgets
        .iter()
        .map(|budget| {
            (
                budget.name.clone(),
                budget.period.clone(),
                budget.cap_usd.clone(),
                budget.spent_usd.clone(),
                budget.remaining_usd.clone(),
                budget.unpriced_tokens,
            )
        })
        .collect();
    assert_eq!(
        budgets,
        vec![
            (
                "billing".into(),
                "day".into(),
                None,
                "0.000201".into(),
                None,
                5
            ),
            (
                "billing".into(),
                "month".into(),
                Some("0.001000".into()),
                "0.000201".into(),
                Some("0.000799".into()),
                5
            ),
            (
                "field".into(),
                "day".into(),
                None,
                "0.000001".into(),
                None,
                0
            ),
            (
                "field".into(),
                "month".into(),
                None,
                "0.000001".into(),
                None,
                0
            ),
        ]
    );
    let all = p
        .usage_report(usage_query(&b, b.owner, UsageGrouping::Department, None))
        .await
        .unwrap();
    assert_eq!(
        departments(&all),
        ["Deleted department", "billing", "field", "support"]
    );
    assert!(all.prices_editable);
    // The drill-down: Billing's calls by the run's desk.
    let desks = p
        .usage_report(usage_query(&b, lead, UsageGrouping::Desk, Some(b.billing)))
        .await
        .unwrap();
    assert_eq!(desks.groups.len(), 1);
    assert_eq!(desks.groups[0].desk.as_ref().unwrap().name, "Unit desk");
    assert_eq!(desks.groups[0].calls, 2);
    // A lead reads nothing outside their units and cannot change prices.
    let outside = p
        .usage_report(usage_query(&b, lead, UsageGrouping::Desk, Some(b.support)))
        .await
        .unwrap();
    assert!(outside.groups.is_empty());
    let price = p
        .save_model_price(PriceWrite {
            scope: UsageScope {
                company: b.company,
                actor: lead,
            },
            provider: "openai".into(),
            model: "fixture".into(),
            expected_revision: None,
            rates: Some(crate::services::model_usage::PriceRates {
                input: crate::services::model_usage::TokenPrice::from_micros(0).unwrap(),
                output: crate::services::model_usage::TokenPrice::from_micros(0).unwrap(),
            }),
        })
        .await;
    assert!(matches!(price, Err(AppError::NotFound(_))), "{price:?}");
    // The dashboard panel reads the same totals.
    let (spend, _) = p
        .unit_spend(UsageScope {
            company: b.company,
            actor: lead,
        })
        .await
        .unwrap();
    assert_eq!(spend.len(), 4);
    let stranger = p
        .unit_spend(UsageScope {
            company: b.company,
            actor: Uuid::new_v4(),
        })
        .await;
    assert!(matches!(stranger, Err(AppError::NotFound(_))));
}
