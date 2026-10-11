use super::*;
use sqlx::{PgConnection, Postgres, pool::PoolConnection};
use std::sync::atomic::AtomicBool;

pub(in super::super) struct Gate {
    connection: PoolConnection<Postgres>,
    pub(in super::super) class: i32,
    pub(in super::super) key: i32,
    pid: i32,
}
#[derive(Debug, sqlx::FromRow)]
pub(in super::super) struct Owner {
    pub(in super::super) pid: i32,
    pub(in super::super) database: String,
    xact_start: chrono::DateTime<chrono::Utc>,
}
impl Gate {
    pub(in super::super) async fn hold(f: &AdmissionFixture) -> Self {
        let mut connection = f.persistence().pool().acquire().await.unwrap();
        // A failed SQL unlock or panic must never return a session lock to the pool.
        connection.close_on_drop();
        let pid = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *connection)
            .await
            .unwrap();
        let key = (Uuid::new_v4().as_u128() & 0x3fff_ffff) as i32 + 1;
        let class = 47_003;
        sqlx::query("SELECT pg_advisory_lock($1,$2)")
            .bind(class)
            .bind(key)
            .execute(&mut *connection)
            .await
            .unwrap();
        Self {
            connection,
            class,
            key,
            pid,
        }
    }

    pub(in super::super) async fn run(
        &mut self,
        fixture: &AdmissionFixture,
        command: &ReconcileActionCommand,
        verifier: Arc<dyn ActionEvidenceVerifier>,
    ) -> (
        AppResult<ReconciliationResult>,
        Result<Option<Owner>, String>,
    ) {
        let done = AtomicBool::new(false);
        let service = async {
            let result = Box::pin(run_command(fixture, command, verifier)).await;
            done.store(true, Ordering::SeqCst);
            result
        };
        let observe = async {
            let observed = tokio::time::timeout(Duration::from_secs(2), self.observe(&done)).await;
            // Release even on timeout/query failure; join below always drains the service.
            let unlock = sqlx::query_scalar::<_, bool>("SELECT pg_advisory_unlock($1,$2)")
                .bind(self.class)
                .bind(self.key)
                .fetch_one(&mut *self.connection)
                .await;
            if !matches!(unlock, Ok(true)) {
                // Discarding this backend releases its session lock if SQL unlock failed.
                return Err(format!("gate release failed: {unlock:?}"));
            }
            observed.map_err(|error| format!("observer gate timeout: {error}"))?
        };
        let (result, observed) = tokio::join!(service, observe);
        if let Ok(Some(owner)) = &observed
            && let Err(error) = quiescent(&mut self.connection, owner, self.pid).await
        {
            return (result, Err(error));
        }
        (result, observed)
    }

    async fn observe(&mut self, done: &AtomicBool) -> Result<Option<Owner>, String> {
        loop {
            sqlx::query("SELECT pg_stat_clear_snapshot()")
                .execute(&mut *self.connection)
                .await
                .map_err(|error| error.to_string())?;
            let owners: Vec<Owner> = sqlx::query_as(
                "SELECT activity.pid,activity.datname AS database,activity.xact_start
                 FROM pg_locks AS waiter JOIN pg_stat_activity AS activity ON activity.pid=waiter.pid
                 WHERE waiter.locktype='advisory' AND waiter.database=(SELECT oid FROM pg_database WHERE datname=current_database())
                   AND waiter.classid=$1::bigint::oid AND waiter.objid=$2::bigint::oid AND waiter.objsubid=2
                   AND NOT waiter.granted AND waiter.mode='ExclusiveLock' AND activity.datname=current_database()
                   AND activity.wait_event_type='Lock' AND activity.wait_event='advisory'
                   AND pg_blocking_pids(activity.pid)=ARRAY[$3]::integer[]")
                .bind(i64::from(self.class)).bind(i64::from(self.key)).bind(self.pid)
                .fetch_all(&mut *self.connection).await.map_err(|error| error.to_string())?;
            if owners.len() > 1 {
                return Err("multiple exact fixture waiters".into());
            }
            if let Some(owner) = owners.into_iter().next() {
                if owner.pid == self.pid {
                    return Err("observer cannot own settlement".into());
                }
                eprintln!(
                    "row3e reached gate owner={} observer={} database={} xact_start={} class={} key={}",
                    owner.pid, self.pid, owner.database, owner.xact_start, self.class, self.key
                );
                return Ok(Some(owner));
            }
            if done.load(Ordering::SeqCst) {
                return Ok(None);
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}

async fn quiescent(
    connection: &mut PgConnection,
    owner: &Owner,
    observer: i32,
) -> Result<(), String> {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            sqlx::query("SELECT pg_stat_clear_snapshot()").execute(&mut *connection).await.map_err(|error| error.to_string())?;
            let clean: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM pg_stat_activity AS activity WHERE activity.pid=$1 AND (activity.datname IS DISTINCT FROM $2 OR activity.state IS DISTINCT FROM 'idle' OR activity.xact_start IS NOT NULL))")
                .bind(owner.pid).bind(&owner.database).fetch_one(&mut *connection).await.map_err(|error| error.to_string())?;
            if clean {
                eprintln!("row3e quiescent owner={} observer={observer} database={}",owner.pid,owner.database);
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.map_err(|error| format!("backend not quiescent: {error}"))?
}

pub(in super::super) async fn catalog(f: &AdmissionFixture) -> Value {
    sqlx::query_scalar(include_str!("action_reconciliation_commit_catalog.sql"))
        .fetch_one(f.persistence().pool())
        .await
        .unwrap()
}

pub(in super::super) fn assert_installation(native: &Value, installed: &Value) {
    let mut remaining = installed.as_object().unwrap().clone();
    let functions = [
        "fixture_commit_require",
        "fixture_commit_public",
        "fixture_commit_sorted",
        "fixture_commit_before",
        "fixture_commit_delta",
        "fixture_commit_applied",
        "fixture_commit_statement",
    ];
    let triggers = [
        "workflow_action_evidence.fixture_commit_before",
        "workflow_action_evidence_commands.fixture_commit_statement",
    ];
    let constraints = [
        "fixture_commit_config.fixture_commit_config_mode_check",
        "fixture_commit_config.fixture_commit_config_pkey",
        "fixture_commit_initial.fixture_commit_initial_pkey",
        "fixture_commit_observed.fixture_commit_observed_pkey",
    ];
    for name in functions {
        let keys: Vec<_> = remaining
            .iter()
            .filter(|(key, row)| key.starts_with("function:") && row["name"] == name)
            .map(|(key, _)| key.clone())
            .collect();
        assert_eq!(keys.len(), 1, "exact fixture function {name}");
        let row = remaining.remove(&keys[0]).unwrap();
        assert!(
            row["definition"]
                .as_str()
                .unwrap()
                .contains("CREATE OR REPLACE FUNCTION public.fixture_commit_")
        );
    }
    for name in triggers {
        let row = remaining
            .remove(&format!("trigger:{name}"))
            .expect("installed fixture trigger");
        assert_eq!(row["enabled"], "O");
        assert_eq!(row["deferred"], false);
        let definition = row["definition"].as_str().unwrap();
        if name.ends_with("before") {
            assert!(definition.contains("BEFORE INSERT") && definition.contains("FOR EACH ROW"));
        } else {
            assert!(
                definition.contains("AFTER INSERT")
                    && definition
                        .contains("REFERENCING NEW TABLE AS inserted_command FOR EACH STATEMENT")
            );
        }
    }
    for name in constraints {
        remaining
            .remove(&format!("constraint:{name}"))
            .expect("installed fixture constraint");
    }
    remove_fixture_not_null(&mut remaining);
    assert_eq!(
        Value::Object(remaining),
        *native,
        "every native constraint/trigger/function unchanged"
    );
}

fn remove_fixture_not_null(remaining: &mut serde_json::Map<String, Value>) {
    for (table, columns) in [
        (
            "fixture_commit_config",
            &[
                "company_id",
                "command_key",
                "mode",
                "gate_class",
                "gate_key",
                "expected",
                "baseline",
            ][..],
        ),
        (
            "fixture_commit_initial",
            &["company_id", "command_key", "original_new", "returned_new"][..],
        ),
        (
            "fixture_commit_observed",
            &[
                "company_id",
                "command_key",
                "owner_pid",
                "database_name",
                "proposed",
            ][..],
        ),
    ] {
        for column in columns {
            let key = format!("constraint:{table}.{table}_{column}_not_null");
            let row = remaining.remove(&key).expect("fixture NOT NULL constraint");
            assert_eq!(row["definition"], format!("NOT NULL {column}"));
        }
    }
}
