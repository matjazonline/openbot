use super::*;
use crate::services::model_usage::*;

pub(super) async fn scope(f: &Fixture) -> UsageScope {
    UsageScope {
        company: f.identity.company_id,
        actor: sqlx::query_scalar("SELECT user_id FROM companies WHERE id=$1")
            .bind(f.identity.company_id)
            .fetch_one(&f.persistence.pool)
            .await
            .unwrap(),
    }
}
pub(super) fn query(scope: UsageScope) -> UsageQuery {
    UsageQuery {
        scope,
        start: Utc::now() - chrono::Duration::days(1),
        end: Utc::now() + chrono::Duration::minutes(1),
        grouping: UsageGrouping::Model,
        agent: None,
        workflow: None,
    }
}
fn turn(reservation: &ModelReservation) -> SavedModelTurn {
    SavedModelTurn {
        request_id: reservation.request_id,
        input_tokens: 12,
        output_tokens: 7,
        token_usage_source: TokenUsageSource::Mixed,
        invalid_response: None,
        text: "private".into(),
        calls: vec![],
        continuation: vec![],
    }
}
#[tokio::test]
async fn model_usage_reservation_settlement_replay_and_current_prices() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = Fixture::new(database.pool.clone()).await;
    let scope = scope(&f).await;
    let mut run = applied(f.persistence.open_run(f.request()).await.unwrap());
    let reservation = reservation();
    let fence = write(&f, &run);
    run = applied(
        f.persistence
            .reserve_model(&fence, reservation.clone())
            .await
            .unwrap(),
    );
    assert!(matches!(
        f.persistence
            .reserve_model(&fence, reservation.clone())
            .await
            .unwrap(),
        WriteOutcome::AlreadyApplied(_)
    ));
    let report = f.persistence.usage_report(query(scope)).await.unwrap();
    let group = &report.groups[0];
    assert_eq!(
        (
            group.calls,
            group.unconfirmed_calls,
            group.input_tokens,
            group.output_tokens
        ),
        (1, 1, 100, 100)
    );
    assert_eq!(group.priced_subtotal_usd, None);
    let fence = write(&f, &run);
    run = applied(
        f.persistence
            .commit_model_turn(&fence, turn(&reservation))
            .await
            .unwrap(),
    );
    assert!(matches!(
        f.persistence
            .commit_model_turn(&fence, turn(&reservation))
            .await
            .unwrap(),
        WriteOutcome::AlreadyApplied(_)
    ));
    let mut changed = turn(&reservation);
    changed.input_tokens += 1;
    assert!(matches!(
        f.persistence
            .commit_model_turn(&fence, changed)
            .await
            .unwrap(),
        WriteOutcome::RevisionConflict
    ));
    check_current_prices(&f, scope).await;
    assert_eq!(run.turns.len(), 1);
    f.cleanup().await;
}

async fn check_current_prices(f: &Fixture, scope: UsageScope) {
    let price = PriceWrite {
        scope,
        provider: "openai".into(),
        model: "fixture".into(),
        expected_revision: None,
        rates: Some(PriceRates {
            input: TokenPrice::parse("1.25").unwrap(),
            output: TokenPrice::parse("10").unwrap(),
        }),
    };
    f.persistence.save_model_price(price.clone()).await.unwrap();
    assert!(matches!(
        f.persistence.save_model_price(price.clone()).await,
        Err(AppError::Conflict(_))
    ));
    let report = f.persistence.usage_report(query(scope)).await.unwrap();
    let group = &report.groups[0];
    assert_eq!(
        (
            group.calls,
            group.completed_calls,
            group.mixed_calls,
            group.reported_calls,
            group.input_tokens,
            group.output_tokens
        ),
        (1, 1, 1, 0, 12, 7)
    );
    assert_eq!(group.priced_subtotal_usd.as_deref(), Some("0.000085000000"));
    let mut zero = price.clone();
    zero.expected_revision = Some(report.prices[0].revision);
    zero.rates = Some(PriceRates {
        input: TokenPrice::parse("0").unwrap(),
        output: TokenPrice::parse("0").unwrap(),
    });
    let (left, right) = tokio::join!(
        f.persistence.save_model_price(zero.clone()),
        f.persistence.save_model_price(zero)
    );
    assert_ne!(left.is_ok(), right.is_ok());
    let report = f.persistence.usage_report(query(scope)).await.unwrap();
    assert_eq!(
        report.groups[0].priced_subtotal_usd.as_deref(),
        Some("0.000000000000")
    );
    let mut remove = price.clone();
    remove.expected_revision = Some(report.prices[0].revision);
    remove.rates = None;
    f.persistence
        .save_model_price(remove.clone())
        .await
        .unwrap();
    f.persistence.save_model_price(price).await.unwrap();
    assert!(
        matches!(
            f.persistence.save_model_price(remove).await,
            Err(AppError::Conflict(_))
        ),
        "delete/recreate must not reuse a revision"
    );
}

#[tokio::test]
async fn model_usage_failure_rolls_back_checkpoint_and_sql_guards_truth() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = Fixture::new(database.pool.clone()).await;
    let run = applied(f.persistence.open_run(f.request()).await.unwrap());
    let reserved = reservation();
    let run = applied(
        f.persistence
            .reserve_model(&write(&f, &run), reserved.clone())
            .await
            .unwrap(),
    );
    sqlx::raw_sql("CREATE FUNCTION reject_usage_settlement() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fixture'; END; $$; CREATE TRIGGER reject_usage BEFORE UPDATE ON model_usage_calls FOR EACH ROW EXECUTE FUNCTION reject_usage_settlement();")
        .execute(&f.persistence.pool).await.unwrap();
    assert!(
        f.persistence
            .commit_model_turn(&write(&f, &run), turn(&reserved))
            .await
            .is_err()
    );
    let saved: serde_json::Value =
        sqlx::query_scalar("SELECT checkpoint FROM task_harness_runs WHERE id=$1")
            .bind(run.run_id.0)
            .fetch_one(&f.persistence.pool)
            .await
            .unwrap();
    assert_eq!(serde_json::from_value::<RunCheckpoint>(saved).unwrap(), run);
    sqlx::query("DROP TRIGGER reject_usage ON model_usage_calls")
        .execute(&f.persistence.pool)
        .await
        .unwrap();
    f.persistence
        .commit_model_turn(&write(&f, &run), turn(&reserved))
        .await
        .unwrap();
    for assignment in [
        "input_tokens=13",
        "model='changed'",
        "reserved_input=101",
        "usage_source='reported'",
        "settled_at=NULL,input_tokens=NULL,output_tokens=NULL,usage_source=NULL",
    ] {
        let error = sqlx::query(&format!(
            "UPDATE model_usage_calls SET {assignment} WHERE company_id=$1"
        ))
        .bind(f.identity.company_id)
        .execute(&f.persistence.pool)
        .await
        .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
    }
    f.cleanup().await;
}

#[tokio::test]
async fn model_usage_current_authority_tenant_filters_and_bounded_reports() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = Fixture::new(database.pool.clone()).await;
    let other = Fixture::new(database.pool.clone()).await;
    let owner = scope(&f).await;
    let stranger = UsageScope {
        company: owner.company,
        actor: scope(&other).await.actor,
    };
    assert!(matches!(
        f.persistence.usage_report(query(stranger)).await,
        Err(AppError::NotFound(_))
    ));
    let price = PriceWrite {
        scope: stranger,
        provider: "openai".into(),
        model: "fixture".into(),
        expected_revision: None,
        rates: Some(PriceRates {
            input: TokenPrice::parse("1").unwrap(),
            output: TokenPrice::parse("1").unwrap(),
        }),
    };
    assert!(matches!(
        f.persistence.save_model_price(price.clone()).await,
        Err(AppError::NotFound(_))
    ));
    for filter in [
        UsageQuery {
            agent: Some(other.identity.agent_id),
            ..query(owner)
        },
        UsageQuery {
            workflow: Some(Uuid::new_v4()),
            ..query(owner)
        },
    ] {
        assert!(matches!(
            f.persistence.usage_report(filter).await,
            Err(AppError::NotFound(_))
        ));
    }
    sqlx::query("INSERT INTO company_members(id,company_id,user_id,role) VALUES($1,$2,$3,'admin')")
        .bind(Uuid::new_v4())
        .bind(owner.company)
        .bind(stranger.actor)
        .execute(&f.persistence.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO principals(id,company_id,kind,user_id,display_label) VALUES($1,$2,'person',$3,'usage manager')")
        .bind(Uuid::new_v4()).bind(owner.company).bind(stranger.actor).execute(&f.persistence.pool).await.unwrap();
    f.persistence.save_model_price(price.clone()).await.unwrap();
    assert!(f.persistence.usage_report(query(stranger)).await.is_ok());
    sqlx::query("UPDATE company_members SET role='member' WHERE company_id=$1 AND user_id=$2")
        .bind(owner.company)
        .bind(stranger.actor)
        .execute(&f.persistence.pool)
        .await
        .unwrap();
    assert!(matches!(
        f.persistence.usage_report(query(stranger)).await,
        Err(AppError::NotFound(_))
    ));
    assert!(matches!(
        f.persistence.save_model_price(price).await,
        Err(AppError::NotFound(_))
    ));
    check_bounded_reports(&f, &other, owner).await;
    other.cleanup().await;
    f.cleanup().await;
}

async fn check_bounded_reports(f: &Fixture, other: &Fixture, owner: UsageScope) {
    sqlx::query("INSERT INTO model_usage_calls(company_id,request_id,task_id,agent_id,provider,model,reserved_input,reserved_output)
        SELECT $1,gen_random_uuid(),$2,$3,'openai','fixture',100,50 FROM generate_series(1,10001)")
        .bind(owner.company).bind(f.identity.task_id).bind(f.identity.agent_id).execute(&f.persistence.pool).await.unwrap();
    let report = f.persistence.usage_report(query(owner)).await.unwrap();
    assert!(report.truncated);
    assert_eq!(report.scanned_calls, 10000);
    assert_eq!(report.groups[0].calls, 10000);
    assert_eq!(report.groups[0].input_tokens, 1_000_000);
    let workflow = f
        .persistence
        .usage_report(UsageQuery {
            grouping: UsageGrouping::Workflow,
            ..query(owner)
        })
        .await
        .unwrap();
    assert_eq!(workflow.groups[0].workflow, None);
    assert_eq!(workflow.groups[0].calls, 10000);
    let empty = f
        .persistence
        .usage_report(query(scope(other).await))
        .await
        .unwrap();
    assert!(empty.groups.is_empty());
    let mut invalid = query(owner);
    invalid.start = invalid.end - chrono::Duration::days(91);
    assert!(matches!(
        f.persistence.usage_report(invalid).await,
        Err(AppError::BadRequest(_))
    ));
}

#[tokio::test]
async fn model_usage_competing_reservations_write_one_call() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = Fixture::new(database.pool.clone()).await;
    let run = applied(f.persistence.open_run(f.request()).await.unwrap());
    let fence = write(&f, &run);
    let barrier = tokio::sync::Barrier::new(2);
    let reserve = || async {
        barrier.wait().await;
        f.persistence
            .reserve_model(&fence, reservation())
            .await
            .unwrap()
    };
    let (left, right) = tokio::join!(reserve(), reserve());
    assert_ne!(
        matches!(left, WriteOutcome::Applied(_)),
        matches!(right, WriteOutcome::Applied(_))
    );
    let report = f
        .persistence
        .usage_report(query(scope(&f).await))
        .await
        .unwrap();
    assert_eq!(report.groups[0].calls, 1);
    f.cleanup().await;
}

#[tokio::test]
async fn model_usage_over_reservation_settles_terminal_checkpoint_atomically() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = Fixture::new(database.pool.clone()).await;
    let scope = scope(&f).await;
    let mut request = f.request();
    request.policy.total_input_tokens = 100;
    let run = applied(f.persistence.open_run(request).await.unwrap());
    let reservation = reservation();
    let run = applied(
        f.persistence
            .reserve_model(&write(&f, &run), reservation.clone())
            .await
            .unwrap(),
    );
    let fence = write(&f, &run);
    let mut response = turn(&reservation);
    response.input_tokens = 101;
    let run = applied(
        f.persistence
            .commit_model_turn(&fence, response.clone())
            .await
            .unwrap(),
    );
    assert_eq!(run.state, RunState::BudgetExceeded);
    assert!(matches!(
        f.persistence
            .commit_model_turn(&fence, response)
            .await
            .unwrap(),
        WriteOutcome::AlreadyApplied(_)
    ));
    let report = f.persistence.usage_report(query(scope)).await.unwrap();
    assert_eq!(report.groups[0].input_tokens, 101);
    assert_eq!(report.groups[0].unconfirmed_calls, 0);
    assert_eq!(report.groups[0].calls, 1);
    let saved: serde_json::Value =
        sqlx::query_scalar("SELECT checkpoint FROM task_harness_runs WHERE id=$1")
            .bind(run.run_id.0)
            .fetch_one(&f.persistence.pool)
            .await
            .unwrap();
    let restored: RunCheckpoint = serde_json::from_value(saved).unwrap();
    restored.validate().unwrap();
    assert_eq!(restored, run);
    assert!(matches!(
        f.persistence.open_run(f.request()).await,
        Err(AppError::Execution(
            crate::app_error::ExecutionFailure::Budget
        ))
    ));
    f.cleanup().await;
}
