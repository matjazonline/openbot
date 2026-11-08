//! OwnDatabase-only gates inside actual production transactions, never synthetic owner locks.
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

pub(super) enum Target {
    Preparation(Uuid),
    Expiry(Uuid),
    Retry(Uuid),
    Decision(Uuid),
}

struct GateStatement {
    table: &'static str,
    event: &'static str,
    predicate: String,
}

pub(super) struct OwnerGate {
    transaction: Transaction<'static, Postgres>,
    pool: PgPool,
    blocker: i32,
    table: &'static str,
}

impl OwnerGate {
    pub(super) async fn install(pool: &PgPool, target: Target) -> Self {
        let statement = match target {
            Target::Preparation(id) => GateStatement {
                table: "agent_history_exchanges",
                event: "INSERT",
                predicate: format!("NEW.id='{id}'::uuid"),
            },
            Target::Expiry(id) => GateStatement {
                table: "agent_history_exchanges",
                event: "UPDATE",
                predicate: format!(
                    "NEW.id='{id}'::uuid AND OLD.payload_retained AND NOT NEW.payload_retained"
                ),
            },
            Target::Retry(id) => GateStatement {
                table: "background_tasks",
                event: "UPDATE",
                predicate: format!(
                    "NEW.id='{id}'::uuid AND OLD.status='failed' AND NEW.status='pending'"
                ),
            },
            Target::Decision(id) => GateStatement {
                table: "workflow_decisions",
                event: "UPDATE",
                predicate: format!(
                    "NEW.id='{id}'::uuid AND OLD.submitted_at IS NULL AND NEW.submitted_at IS NOT NULL"
                ),
            },
        };
        let GateStatement {
            table,
            event,
            predicate,
        } = statement;
        let mut transaction = pool.begin().await.unwrap();
        let blocker = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *transaction)
            .await
            .unwrap();
        sqlx::query("SELECT pg_advisory_xact_lock(735103)")
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::raw_sql(&format!(
            "CREATE FUNCTION history_test_owner_gate() RETURNS trigger LANGUAGE plpgsql AS $$
             BEGIN IF {predicate} THEN PERFORM pg_advisory_xact_lock(735103); END IF; RETURN NEW; END $$;
             CREATE TRIGGER history_test_owner_gate BEFORE {event} ON {table}
             FOR EACH ROW EXECUTE FUNCTION history_test_owner_gate()"
        )).execute(pool).await.unwrap();
        Self {
            transaction,
            pool: pool.clone(),
            blocker,
            table,
        }
    }

    pub(super) async fn arrived(&self, company: Uuid) -> i32 {
        let owner = Self::blocked_by(&self.pool, self.blocker).await;
        // This lock is held by the actual paused native operation, not by the fixture gate.
        let error = sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE NOWAIT")
            .bind(company)
            .execute(&self.pool)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("55P03")
        );
        owner
    }

    pub(super) async fn blocked_by(pool: &PgPool, blocker: i32) -> i32 {
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                let pid = sqlx::query_scalar::<_, i32>(
                    "SELECT pid FROM pg_stat_activity WHERE datname=current_database()
                     AND $1=ANY(pg_blocking_pids(pid)) LIMIT 1",
                )
                .bind(blocker)
                .fetch_optional(pool)
                .await
                .unwrap();
                if let Some(pid) = pid {
                    return pid;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the actual production owner must reach the fixture gate")
    }

    pub(super) async fn release(self) {
        self.transaction.rollback().await.unwrap();
        sqlx::raw_sql(&format!(
            "DROP TRIGGER history_test_owner_gate ON {}; DROP FUNCTION history_test_owner_gate()",
            self.table
        ))
        .execute(&self.pool)
        .await
        .unwrap();
    }
}
