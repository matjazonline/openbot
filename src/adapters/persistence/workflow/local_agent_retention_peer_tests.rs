use super::*;
use crate::adapters::http::routes::local_machine_connection::tests::tls::TestTlsServer;
use base64::Engine;
use std::{os::unix::fs::PermissionsExt, path::PathBuf, process::Stdio};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, Lines, Take},
    process::{Child, ChildStdin, ChildStdout},
};

struct CountedPeer {
    child: Child,
    input: ChildStdin,
    events: Lines<BufReader<Take<ChildStdout>>>,
    directory: PathBuf,
}

impl CountedPeer {
    async fn spawn(s: &Setup, server: &TestTlsServer, session: SessionId) -> Self {
        let directory =
            std::env::temp_dir().join(format!("openbot-retention-peer-{}", Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        let p = &s.fixture.persistence;
        let (sequence,generation,thread):(i64,i64,String)=sqlx::query_as("SELECT machine.acknowledged_sequence,machine.credential_generation,session.thread_id FROM local_machines AS machine JOIN local_agent_sessions AS session ON session.company_id=machine.company_id AND session.machine_id=machine.id WHERE session.company_id=$1 AND session.id=$2")
            .bind(s.scope.company.as_uuid()).bind(session.as_uuid()).fetch_one(p.pool()).await.unwrap();
        let config = json!({"directory":directory,"paired":{"state":"paired","version":1,
            "binding":{"origin":server.origin,"company_id":s.scope.company.as_uuid(),"machine_id":s.profile.selection.machine_id,
                "installation_id":s.paired.receipt.machine.installation_id,"credential_generation":generation,"journal_id":s.journal_id},
            "credential":s.paired.candidate.credential.expose_secret()},"selection":s.profile.selection,"session":session,"thread":thread,"sequence":sequence});
        let config_path = directory.join("config.json");
        std::fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
        std::fs::set_permissions(&config_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let encoded = base64::engine::general_purpose::STANDARD.encode(server.certificate.as_ref());
        let pem = format!("-----BEGIN CERTIFICATE-----\n{encoded}\n-----END CERTIFICATE-----\n");
        let ca = directory.join("ca.pem");
        std::fs::write(&ca, pem).unwrap();
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("integrations/cloudroom-plugin/build-tests/retention-peer.js");
        assert!(
            script.is_file(),
            "build the plugin test entries before the combined lifecycle test"
        );
        let mut child = tokio::process::Command::new("node")
            .arg(script)
            .arg(config_path)
            .env("NODE_EXTRA_CA_CERTS", ca)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let events = BufReader::new(child.stdout.take().unwrap().take(64 * 1024)).lines();
        Self {
            child,
            input,
            events,
            directory,
        }
    }

    async fn command(&mut self, command: &str) {
        let line = format!("{}\n", json!({"command":command}));
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            self.input.write_all(line.as_bytes()),
        )
        .await
        .unwrap()
        .unwrap();
    }

    async fn event(&mut self, wanted: &str) -> Value {
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let line = self
                    .events
                    .next_line()
                    .await
                    .unwrap()
                    .expect("counted peer exited before its event");
                let value: Value = serde_json::from_str(&line).unwrap();
                assert_ne!(value["event"], "failure", "counted peer failed");
                if value["event"] == wanted {
                    return value;
                }
            }
        })
        .await
        .unwrap()
    }

    async fn close(mut self) {
        self.command("close").await;
        let status = tokio::time::timeout(std::time::Duration::from_secs(2), self.child.wait())
            .await
            .unwrap()
            .unwrap();
        assert!(status.success());
    }

    async fn settled_snapshot(&mut self) -> Value {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                self.command("snapshot").await;
                let snapshot = self.event("snapshot").await;
                if snapshot["outbox"] == 0 {
                    return snapshot;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap()
    }
}
impl Drop for CountedPeer {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

async fn wait_for_state(s: &Setup, run: RunId, expected: &str) {
    tokio::time::timeout(std::time::Duration::from_secs(8), async {
        loop {
            let state: String =
                sqlx::query_scalar("SELECT state FROM workflow_runs WHERE company_id=$1 AND id=$2")
                    .bind(s.scope.company.as_uuid())
                    .bind(run.as_uuid())
                    .fetch_one(s.fixture.persistence.pool())
                    .await
                    .unwrap();
            if state == expected {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

async fn wait_for_disconnect(s: &Setup) {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let disconnected: bool = sqlx::query_scalar(
                "SELECT connection_owner IS NULL FROM local_machines WHERE company_id=$1 AND id=$2",
            )
            .bind(s.scope.company.as_uuid())
            .bind(s.profile.selection.machine_id.as_uuid())
            .fetch_one(s.fixture.persistence.pool())
            .await
            .unwrap();
            if disconnected {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn cloudroom_lifecycle_real_server_plugin_retention_preserves_prepared_and_unseen_queued_work_once()
 {
    let s = Setup::new().await;
    let p = &s.fixture.persistence;
    let approval = json!({"reviewers":[{"kind":"user","id":s.fixture.target.actor.user_id()}],"deadline":(chrono::Utc::now()+chrono::Duration::minutes(20)).to_rfc3339()});
    let a = waiting_root(&s, Some(approval)).await;
    let b = waiting_root(&s, None).await;
    let follow = target(&s, &a, b.session).await;
    let claim = p
        .claim_io(follow, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        s.handler()
            .execute(&WorkflowStepKind(KIND.into()), &claim)
            .await,
        WorkflowHandlerOutcome::Suspended(_)
    ));
    cancel(&s, b.scope, "combined-old-original").await;
    age(&s, &[b.scope]).await;
    let server = TestTlsServer::start(Arc::new(p.clone())).await;
    let mut peer = CountedPeer::spawn(&s, &server, b.session).await;
    peer.command("start").await;
    peer.event("started").await;
    assert_eq!(peer.event("retirement").await["retired"], json!([]));
    p.retain_local_agents(None).await.unwrap();
    assert!(
        p.retire_tree(retention_root(&s, b.scope).await)
            .await
            .unwrap()
            .is_none()
    );
    peer.command("stop").await;
    peer.event("stopped").await;
    wait_for_disconnect(&s).await;
    let command = super::super::super::approval_test_support::command(
        p,
        follow,
        s.fixture.target.actor,
        "approve",
    )
    .await;
    p.submit_decision(command).await.unwrap();
    let claim = p
        .claim_io(follow, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    s.park(&claim).await;
    let unseen:bool=sqlx::query_scalar("SELECT operation.delivery_attempted_at IS NULL FROM local_agent_operations AS operation JOIN workflow_local_agent_calls AS call ON call.company_id=operation.company_id AND call.operation_id=operation.id WHERE call.company_id=$1 AND call.execution_id=$2")
        .bind(follow.company.as_uuid()).bind(follow.execution.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert!(
        unseen,
        "the approved Send must still be unseen by the stopped plugin"
    );
    p.retain_local_agents(None).await.unwrap();
    peer.command("snapshot").await;
    let snapshot = peer.event("snapshot").await;
    assert_eq!(
        (snapshot["mapped"].clone(), snapshot["calls"].clone()),
        (json!(true), json!(0))
    );
    peer.command("start").await;
    peer.event("started").await;
    wait_for_state(&s, a.scope.run, "waiting").await;
    tokio::time::timeout(std::time::Duration::from_secs(8),async {
        loop {
            let ready:bool=sqlx::query_scalar("SELECT ready_at IS NOT NULL FROM workflow_local_agent_calls WHERE company_id=$1 AND execution_id=$2")
                .bind(follow.company.as_uuid()).bind(follow.execution.as_uuid()).fetch_one(p.pool()).await.unwrap();
            if ready {return;}
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    assert_eq!(consume_local(&s, follow).await.0, b.session);
    let snapshot = peer.settled_snapshot().await;
    assert_eq!(snapshot["calls"], 1);
    peer.command("stop").await;
    peer.event("stopped").await;
    age(&s, &[a.scope]).await;
    p.retain_local_agents(None).await.unwrap();
    peer.command("age").await;
    peer.event("aged").await;
    peer.command("start").await;
    peer.event("started").await;
    assert_eq!(
        peer.event("retirement").await["retired"],
        json!([b.session])
    );
    peer.command("snapshot").await;
    let snapshot = peer.event("snapshot").await;
    assert_eq!(
        (snapshot["mapped"].clone(), snapshot["calls"].clone()),
        (json!(false), json!(1))
    );
    peer.close().await;
    server.close().await;
}
