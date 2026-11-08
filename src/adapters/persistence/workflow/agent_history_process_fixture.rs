use super::*;
use std::io::{BufRead, Read, Write};
use std::process::{Child, Command, Stdio};

#[derive(Debug, Deserialize)]
pub(super) struct ClaimMark {
    worker: Uuid,
    generation: Uuid,
    attempt: i32,
}
impl ClaimMark {
    pub(super) fn fence(&self, scope: ActivationRequest) -> WorkflowFence {
        WorkflowFence {
            scope,
            worker: WorkflowWorkerId(self.worker),
            generation: WorkflowGeneration(self.generation),
            attempt: WorkflowAttempt(self.attempt),
        }
    }
}

pub(super) struct WorkerProcess {
    child: Option<Child>,
    lines: tokio::sync::mpsc::Receiver<String>,
    reader: Option<std::thread::JoinHandle<Result<(), &'static str>>>,
    observed: Vec<String>,
}
impl WorkerProcess {
    pub(super) fn spawn(spec: &ProcessSpec) -> Self {
        spec.validate_fixture();
        let parent = module_path!()
            .split_once("::")
            .unwrap()
            .1
            .trim_end_matches("::process");
        let name = format!("{parent}::driver::history_crash_child_entry");
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                &name,
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(
                "AGENT_HISTORY_PROCESS_SPEC",
                serde_json::to_string(spec).unwrap(),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, lines) = tokio::sync::mpsc::channel(32);
        let reader = std::thread::spawn(move || read_lines(stdout, sender));
        Self {
            child: Some(child),
            lines,
            reader: Some(reader),
            observed: Vec::new(),
        }
    }
    pub(super) async fn mark(&mut self, marker: &str) -> Value {
        let result = tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(line) = self.lines.recv().await {
                self.observed.push(line.clone());
                if let Some((_, value)) = line.split_once(marker) {
                    return serde_json::from_str(value.trim()).unwrap();
                }
            }
            panic!("worker exited before {marker}: {:?}", self.observed);
        })
        .await;
        result.unwrap_or_else(|_| panic!("worker never reached {marker}: {:?}", self.observed))
    }
    pub(super) fn proceed(&mut self) {
        let input = self.child.as_mut().unwrap().stdin.as_mut().unwrap();
        input.write_all(b"continue\n").unwrap();
        input.flush().unwrap();
    }
    pub(super) fn kill(mut self) {
        let mut child = self.child.take().unwrap();
        child
            .kill()
            .expect("actual live worker receives process kill");
        let status = child.wait().unwrap();
        assert!(!status.success());
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            assert_eq!(
                status.signal(),
                Some(9),
                "a real SIGKILL, not a returned/aborted future"
            );
        }
        self.lines.close();
        self.reader.take().unwrap().join().unwrap().unwrap();
    }
}
impl Drop for WorkerProcess {
    fn drop(&mut self) {
        self.lines.close();
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn read_lines(
    stdout: std::process::ChildStdout,
    sender: tokio::sync::mpsc::Sender<String>,
) -> Result<(), &'static str> {
    let mut input = std::io::BufReader::new(stdout);
    for _ in 0..128 {
        let mut line = Vec::new();
        Read::by_ref(&mut input)
            .take(8193)
            .read_until(b'\n', &mut line)
            .map_err(|_| "worker output read failed")?;
        if line.is_empty() {
            return Ok(());
        }
        if line.len() > 8192 {
            return Err("worker output line exceeded bound");
        }
        let line = String::from_utf8(line).map_err(|_| "invalid worker output")?;
        if sender.blocking_send(line).is_err() {
            return Ok(());
        }
    }
    Err("worker output exceeded line count")
}

pub(super) struct StatementGate {
    transaction: sqlx::Transaction<'static, sqlx::Postgres>,
    pool: sqlx::PgPool,
    blocker: i32,
    table: &'static str,
}
struct Hook {
    table: &'static str,
    timing: &'static str,
    predicate: String,
}
impl StatementGate {
    pub(super) async fn install(pool: &sqlx::PgPool, spec: &ProcessSpec) -> Option<Self> {
        let target = spec.execution;
        let hook = match spec.point {
            CrashPoint::Acquisition => Hook {
                table: "agent_history_conversations",
                timing: "AFTER",
                predicate: format!(
                    "NEW.active_exchange_id='{target}'::uuid AND OLD.active_exchange_id IS DISTINCT FROM NEW.active_exchange_id"
                ),
            },
            CrashPoint::ModelSettled => Hook {
                table: "agent_history_exchanges",
                timing: "BEFORE",
                predicate: format!(
                    "NEW.id='{target}'::uuid AND OLD.state='active' AND NEW.state='completed'"
                ),
            },
            _ => return None,
        };
        let mut transaction = pool.begin().await.unwrap();
        let blocker = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *transaction)
            .await
            .unwrap();
        sqlx::query("SELECT pg_advisory_xact_lock(735107)")
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::raw_sql(&format!("CREATE FUNCTION history_process_gate() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF {} THEN PERFORM pg_advisory_xact_lock(735107); END IF; RETURN NEW; END $$; CREATE TRIGGER history_process_gate {} UPDATE ON {} FOR EACH ROW EXECUTE FUNCTION history_process_gate()",hook.predicate,hook.timing,hook.table)).execute(pool).await.unwrap();
        Some(Self {
            transaction,
            pool: pool.clone(),
            blocker,
            table: hook.table,
        })
    }
    pub(super) async fn arrived(&self, company: CompanyId) {
        crate::adapters::persistence::workflow::agent_history_race_fixture::OwnerGate::blocked_by(
            &self.pool,
            self.blocker,
        )
        .await;
        let error = sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE NOWAIT")
            .bind(company.as_uuid())
            .execute(&self.pool)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("55P03")
        );
    }
    pub(super) async fn release(self) {
        self.transaction.rollback().await.unwrap();
        sqlx::raw_sql(&format!(
            "DROP TRIGGER history_process_gate ON {}; DROP FUNCTION history_process_gate()",
            self.table
        ))
        .execute(&self.pool)
        .await
        .unwrap();
    }
}
