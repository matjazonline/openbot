use super::*;
use crate::services::model_usage::*;
use std::time::Duration;

#[derive(Clone, Copy)]
enum Operation {
    Report,
    Price,
}
impl Operation {
    async fn run(
        self,
        persistence: &PostgresPersistence,
        scope: UsageScope,
    ) -> crate::app_error::AppResult<()> {
        match self {
            Self::Report => persistence
                .usage_report(usage_writes::query(scope))
                .await
                .map(|_| ()),
            Self::Price => {
                persistence
                    .save_model_price(PriceWrite {
                        scope,
                        provider: "openai".into(),
                        model: "authority-race".into(),
                        expected_revision: None,
                        rates: Some(PriceRates {
                            input: TokenPrice::parse("1").unwrap(),
                            output: TokenPrice::parse("2").unwrap(),
                        }),
                    })
                    .await
            }
        }
    }
}

async fn await_blocked(pool: &sqlx::PgPool, blocker: i32) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let blocked: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND $1=ANY(pg_blocking_pids(pid)))",
            ).bind(blocker).fetch_one(pool).await.unwrap();
            if blocked { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("usage operation must reach the controlled database lock");
}

async fn demote(
    tx: &mut sqlx::PgConnection,
    scope: UsageScope,
) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    sqlx::query("UPDATE company_members SET role='member' WHERE company_id=$1 AND user_id=$2")
        .bind(scope.company)
        .bind(scope.actor)
        .execute(tx)
        .await
}

async fn operation_first(f: &Fixture, scope: UsageScope, operation: Operation) {
    let mut blocker = f.persistence.pool.begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    // Both the report SELECT and price INSERT reach this table after authorization.
    sqlx::query("LOCK TABLE model_prices IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let observe = async {
        await_blocked(&f.persistence.pool, pid).await;
        let mut revoke = f.persistence.pool.begin().await.unwrap();
        sqlx::query("SET LOCAL lock_timeout='150ms'")
            .execute(&mut *revoke)
            .await
            .unwrap();
        let error = demote(&mut revoke, scope).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("55P03")
        );
        revoke.rollback().await.unwrap();
        blocker.commit().await.unwrap();
    };
    let (result, ()) = tokio::join!(operation.run(&f.persistence, scope), observe);
    result.unwrap();
    let mut revoke = f.persistence.pool.begin().await.unwrap();
    assert_eq!(demote(&mut revoke, scope).await.unwrap().rows_affected(), 1);
    revoke.commit().await.unwrap();
    assert!(matches!(
        operation.run(&f.persistence, scope).await,
        Err(AppError::NotFound(_))
    ));
}

async fn revocation_first(f: &Fixture, scope: UsageScope, operation: Operation) {
    let mut revoke = f.persistence.pool.begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *revoke)
        .await
        .unwrap();
    assert_eq!(demote(&mut revoke, scope).await.unwrap().rows_affected(), 1);
    let commit = async {
        await_blocked(&f.persistence.pool, pid).await;
        revoke.commit().await.unwrap();
    };
    let (result, ()) = tokio::join!(operation.run(&f.persistence, scope), commit);
    assert!(matches!(result, Err(AppError::NotFound(_))));
}

#[tokio::test]
async fn model_usage_report_and_price_hold_membership_authority_through_operation() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = Fixture::new(database.pool.clone()).await;
    let other = Fixture::new(database.pool.clone()).await;
    let scope = UsageScope {
        company: f.identity.company_id,
        actor: usage_writes::scope(&other).await.actor,
    };
    sqlx::query("INSERT INTO company_members(id,company_id,user_id,role) VALUES($1,$2,$3,'admin')")
        .bind(Uuid::new_v4())
        .bind(scope.company)
        .bind(scope.actor)
        .execute(&f.persistence.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO principals(id,company_id,kind,user_id,display_label) VALUES($1,$2,'person',$3,'usage manager')")
        .bind(Uuid::new_v4()).bind(scope.company).bind(scope.actor).execute(&f.persistence.pool).await.unwrap();
    for operation in [Operation::Report, Operation::Price] {
        for order in 0..2 {
            sqlx::query(
                "UPDATE company_members SET role='admin' WHERE company_id=$1 AND user_id=$2",
            )
            .bind(scope.company)
            .bind(scope.actor)
            .execute(&f.persistence.pool)
            .await
            .unwrap();
            if order == 0 {
                operation_first(&f, scope, operation).await;
            } else {
                revocation_first(&f, scope, operation).await;
            }
        }
    }
    let prices: i64 = sqlx::query_scalar("SELECT count(*) FROM model_prices WHERE company_id=$1")
        .bind(scope.company)
        .fetch_one(&f.persistence.pool)
        .await
        .unwrap();
    assert_eq!(prices, 1, "only the authorized price operation committed");
    other.cleanup().await;
    f.cleanup().await;
}
