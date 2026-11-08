//! Real workers, PostgreSQL and TLS/WSS; the native runtime is a counted fixture.
//! Native Cloudroom/provider acceptance is separate from these deterministic tests.
use super::*;
use crate::adapters::http::routes::local_machine_connection::tests::tls::TestTlsServer;
use crate::application::local_agents::management::{
    ControlSession, LocalAgentManagementStore, SessionCommand,
};
use crate::application::workflow::polling::PollPolicy;
use base64::Engine;
use std::{
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::PathBuf,
    process::Stdio,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, Lines, Take},
    process::{Child, ChildStdin, ChildStdout},
    sync::Notify,
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

struct Workers {
    stop: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
}
impl Workers {
    fn start(p: &PostgresPersistence) -> Self {
        let stop = CancellationToken::new();
        let tasks = (0..2)
            .map(|_| {
                let p = p.clone();
                let shutdown = stop.clone();
                tokio::spawn(async move {
                    let handler = LocalAgentHandler::new(
                        local_agents::PostgresLocalAgentWorkflow::new(
                            p.clone(),
                            Some(super::super::approval_test_support::notices(&p)),
                        ),
                        lease(),
                    );
                    let worker = WorkflowWorker {
                        port: &p,
                        handler: &handler,
                        notices: None,
                        worker: WorkflowWorkerId(Uuid::new_v4()),
                        lease: lease(),
                        poll: PollPolicy::new(std::time::Duration::from_millis(50), 2).unwrap(),
                        active_jobs: None,
                        monitoring: None,
                    };
                    worker.run(&Notify::new(), &shutdown).await;
                })
            })
            .collect();
        Self { stop, tasks }
    }
    async fn close(mut self) {
        self.stop.cancel();
        for task in self.tasks.drain(..) {
            tokio::time::timeout(std::time::Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap();
        }
    }
    async fn crash(mut self) {
        for task in &self.tasks {
            task.abort();
        }
        for task in self.tasks.drain(..) {
            assert!(task.await.unwrap_err().is_cancelled());
        }
    }
}
impl Drop for Workers {
    fn drop(&mut self) {
        self.stop.cancel();
        for task in &self.tasks {
            task.abort();
        }
    }
}

struct Peer {
    child: Child,
    input: ChildStdin,
    events: Lines<BufReader<Take<ChildStdout>>>,
    directory: PathBuf,
}
impl Peer {
    fn new(s: &Setup, server: &TestTlsServer, cut: &str) -> Self {
        let directory = std::env::temp_dir().join(format!("openbot-e2e-peer-{}", Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        let encoded = base64::engine::general_purpose::STANDARD.encode(server.certificate.as_ref());
        std::fs::write(
            directory.join("ca.pem"),
            format!("-----BEGIN CERTIFICATE-----\n{encoded}\n-----END CERTIFICATE-----\n"),
        )
        .unwrap();
        let machine = &s.paired.receipt.machine;
        let config = json!({"directory": directory, "cut": cut, "selection": s.profile.selection,
            "paired":{"state":"paired","version":1,"binding":{"origin":server.origin,
                "company_id":s.scope.company.as_uuid(),"machine_id":machine.id,"installation_id":machine.installation_id,
                "credential_generation":machine.credential_generation,"journal_id":s.journal_id},
                "credential":s.paired.candidate.credential.expose_secret()}});
        std::fs::write(
            directory.join("config.json"),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        std::fs::set_permissions(
            directory.join("config.json"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        Self::spawn(directory)
    }
    fn spawn(directory: PathBuf) -> Self {
        Self::spawn_script(directory, "e2e-peer.js")
    }
    fn spawn_script(directory: PathBuf, name: &str) -> Self {
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("integrations/cloudroom-plugin/build-tests")
            .join(name);
        assert!(
            script.is_file(),
            "build the counted Node e2e peer before Rust tests"
        );
        let mut command = tokio::process::Command::new("node");
        command
            .arg(script)
            .arg(directory.join("config.json"))
            .env("NODE_EXTRA_CA_CERTS", directory.join("ca.pem"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        command.as_std_mut().process_group(0);
        let mut child = command.spawn().unwrap();
        let input = child.stdin.take().unwrap();
        let events = BufReader::new(child.stdout.take().unwrap().take(128 * 1024)).lines();
        Self {
            child,
            input,
            events,
            directory,
        }
    }
    fn event<'a>(&'a mut self, expected: &'a str) -> impl std::future::Future<Output = Value> + 'a {
        self.event_timeout(expected, 15)
    }
    async fn event_timeout(&mut self, expected: &str, seconds: u64) -> Value {
        tokio::time::timeout(std::time::Duration::from_secs(seconds), async {
            loop {
                let line = self
                    .events
                    .next_line()
                    .await
                    .unwrap()
                    .expect("counted peer exited before event");
                let value: Value = serde_json::from_str(&line).unwrap();
                assert_ne!(value["event"], "failure", "counted fixture failure");
                if value["event"] == expected {
                    return value;
                }
            }
        })
        .await
        .unwrap()
    }
    async fn command(&mut self, value: Value) {
        let mut bytes = serde_json::to_vec(&value).unwrap();
        assert!(bytes.len() <= 4096);
        bytes.push(b'\n');
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            self.input.write_all(&bytes),
        )
        .await
        .unwrap()
        .unwrap();
    }
    async fn snapshot(&mut self) -> Value {
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            self.input.write_all(b"{\"command\":\"snapshot\"}\n"),
        )
        .await
        .unwrap()
        .unwrap();
        self.event("snapshot").await
    }
    async fn crashed(&mut self, expected: &str) {
        let status = tokio::time::timeout(std::time::Duration::from_secs(15), self.child.wait())
            .await
            .unwrap()
            .unwrap();
        assert!(!status.success());
        let cut: Value =
            serde_json::from_slice(&std::fs::read(self.directory.join("cut.json")).unwrap())
                .unwrap();
        assert_eq!(cut["stage"], expected);
    }
    fn reopen(&mut self) {
        let directory = self.directory.clone();
        let replacement = Self::spawn(directory);
        // Transfer ownership of the retained fixture directory before dropping the old peer.
        self.directory = PathBuf::new();
        *self = replacement;
    }
    async fn kill(&mut self) {
        self.child.start_kill().unwrap();
        self.child.wait().await.unwrap();
    }
    async fn close(mut self) {
        self.input
            .write_all(b"{\"command\":\"close\"}\n")
            .await
            .unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(3), self.child.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        if let Some(pid) = self.child.id() {
            // The native fixture's launcher shares this owned group; its TERM
            // handler joins its separate server/provider groups on unwinding too.
            let _ = std::process::Command::new("/bin/kill")
                .arg("-TERM")
                .arg(format!("-{pid}"))
                .status();
        }
        let _ = self.child.start_kill();
        if !self.directory.as_os_str().is_empty() {
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }
}

fn state<'a>(s: &'a Setup, expected: &'a str) -> impl std::future::Future<Output = ()> + 'a {
    state_timeout(s, expected, 15)
}
async fn state_timeout(s: &Setup, expected: &str, seconds: u64) {
    tokio::time::timeout(std::time::Duration::from_secs(seconds), async {
        loop {
            let actual: String =
                sqlx::query_scalar("SELECT state FROM workflow_runs WHERE company_id=$1 AND id=$2")
                    .bind(s.scope.company.as_uuid())
                    .bind(s.scope.run.as_uuid())
                    .fetch_one(s.fixture.persistence.pool())
                    .await
                    .unwrap();
            if actual == expected {
                return;
            }
            assert!(
                !matches!(actual.as_str(), "succeeded" | "failed" | "cancelled"),
                "expected workflow {expected}, observed terminal {actual}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}
async fn ready(s: &Setup) -> String {
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            let outcome: Option<String> = sqlx::query_scalar("SELECT workflow_outcome FROM workflow_local_agent_calls WHERE company_id=$1 AND execution_id=$2 AND ready_at IS NOT NULL")
                .bind(s.scope.company.as_uuid()).bind(s.scope.execution.as_uuid()).fetch_optional(s.fixture.persistence.pool()).await.unwrap().flatten();
            if let Some(outcome) = outcome { return outcome; }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }).await.unwrap()
}
async fn parked(s: &Setup) {
    let workers = Workers::start(&s.fixture.persistence);
    state(s, "waiting").await;
    workers.close().await;
    let counts:(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM workflow_local_agent_calls WHERE company_id=$1 AND run_id=$2),(SELECT count(*) FROM local_agent_operations WHERE company_id=$1 AND command->>'kind' IN ('create','send'))")
        .bind(s.scope.company.as_uuid()).bind(s.scope.run.as_uuid()).fetch_one(s.fixture.persistence.pool()).await.unwrap();
    assert_eq!(
        counts,
        (1, 1),
        "competing real workers must park exactly one native invocation"
    );
}
async fn operation(s: &Setup, request: OperationId) {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let outcome: Option<String> = sqlx::query_scalar(
                "SELECT outcome FROM local_agent_operations WHERE company_id=$1 AND id=$2",
            )
            .bind(s.scope.company.as_uuid())
            .bind(request.as_uuid())
            .fetch_one(s.fixture.persistence.pool())
            .await
            .unwrap();
            if let Some(outcome) = outcome {
                assert_eq!(outcome, "succeeded");
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn cloudroom_e2e_competing_workers_native_thread_followup_controls_and_company_fences() {
    let s = Setup::new().await;
    let p = &s.fixture.persistence;
    let server = TestTlsServer::start(Arc::new(p.clone())).await;
    let mut peer = Peer::new(&s, &server, "none");
    peer.event("started").await;
    let workers = Workers::start(p);
    state(&s, "succeeded").await;
    workers.close().await;
    let session: Uuid = sqlx::query_scalar(
        "SELECT session_id FROM workflow_local_agent_calls WHERE company_id=$1 AND run_id=$2",
    )
    .bind(s.scope.company.as_uuid())
    .bind(s.scope.run.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    let session = SessionId::new(session).unwrap();
    for command in [
        SessionCommand::Send {
            prompt: LocalPrompt::try_from("explicit e2e follow-up".to_owned()).unwrap(),
        },
        SessionCommand::Stop,
        SessionCommand::Archive,
    ] {
        let request = ControlSession {
            actor: s.fixture.target.actor.user_id(),
            company: s.scope.company.as_uuid(),
            request_id: OperationId::new(Uuid::now_v7()).unwrap(),
            session_id: session,
            command,
        };
        p.control_session(&request).await.unwrap();
        p.control_session(&request).await.unwrap();
        operation(&s, request.request_id).await;
    }
    let original = p
        .read_session(
            s.fixture.target.actor.user_id(),
            s.scope.company.as_uuid(),
            session,
        )
        .await
        .unwrap();
    assert_eq!(
        original.session.thread_id.as_ref().unwrap().as_str(),
        "thr_owned"
    );
    let snapshot = peer.snapshot().await;
    assert_eq!(
        snapshot["calls"],
        json!(["spawn", "send", "stop", "archive"])
    );
    let receipts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_action_receipts WHERE company_id=$1 AND run_id=$2",
    )
    .bind(s.scope.company.as_uuid())
    .bind(s.scope.run.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(receipts, 1);
    let foreign: Uuid =
        sqlx::query_scalar("SELECT id FROM companies WHERE id<>$1 ORDER BY id LIMIT 1")
            .bind(s.scope.company.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert!(
        p.read_session(s.fixture.target.actor.user_id(), foreign, session)
            .await
            .is_err()
    );
    assert!(
        p.control_session(&ControlSession {
            actor: s.fixture.target.actor.user_id(),
            company: foreign,
            request_id: OperationId::new(Uuid::now_v7()).unwrap(),
            session_id: session,
            command: SessionCommand::Stop
        })
        .await
        .is_err()
    );
    assert_eq!(peer.snapshot().await["calls"], snapshot["calls"]);
    peer.close().await;
    server.close().await;
}

#[tokio::test]
async fn cloudroom_e2e_cut_points_reconstruct_server_plugin_and_workers_without_blind_native_retry()
{
    for cut in [
        "before_receipt",
        "after_receipt",
        "after_intent",
        "after_sdk",
        "after_mapping",
        "after_terminal",
        "after_server_receipt",
        "after_workflow_wake",
    ] {
        let s = Setup::new().await;
        let p = &s.fixture.persistence;
        parked(&s).await;
        let mut server = TestTlsServer::start(Arc::new(p.clone())).await;
        let mut peer = Peer::new(&s, &server, cut);
        peer.event("started").await;
        if cut == "after_workflow_wake" {
            assert_eq!(ready(&s).await, "succeeded");
            peer.kill().await;
        } else {
            peer.crashed(cut).await;
        }
        server
            .restart(
                Arc::new(PostgresPersistence::new(p.pool().clone())),
                axum::Router::new(),
            )
            .await;
        peer.reopen();
        peer.event("started").await;
        let outcome = ready(&s).await;
        let succeeded = matches!(
            cut,
            "after_sdk"
                | "after_mapping"
                | "after_terminal"
                | "after_server_receipt"
                | "after_workflow_wake"
        );
        assert_eq!(outcome == "succeeded", succeeded, "cut {cut}");
        let workers = Workers::start(p);
        state(&s, if succeeded { "succeeded" } else { "failed" }).await;
        workers.close().await;
        let snapshot = peer.snapshot().await;
        assert_eq!(
            snapshot["calls"].as_array().unwrap().len(),
            usize::from(succeeded),
            "cut {cut}: zero or one actual native mutation, never a blind retry"
        );
        let receipt: Value = sqlx::query_scalar(
            "SELECT result FROM workflow_action_receipts WHERE company_id=$1 AND run_id=$2",
        )
        .bind(s.scope.company.as_uuid())
        .bind(s.scope.run.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
        assert_eq!(
            receipt["status"], outcome,
            "the one shared receipt must preserve the actual disposition at {cut}"
        );
        assert_eq!(
            receipt["status"] == "succeeded",
            succeeded,
            "uncertainty cannot turn into success at {cut}"
        );
        peer.close().await;
        server.close().await;
    }
}

#[path = "local_agent_native_e2e_tests.rs"]
mod native;

#[path = "local_agent_early_e2e_tests.rs"]
mod early;
