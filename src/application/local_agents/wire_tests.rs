use super::protocol::JournalObservation;
use crate::domain::local_agents::*;
use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn cloudroom_protocol_rust_node_command_hash_preserves_exact_deadline_and_text() {
    let selection: ExecutionSelection = serde_json::from_value(serde_json::json!({
        "machine_id":uuid::Uuid::new_v4(), "project_id":"proj_wire", "provider":"acp-opencode",
        "model":"openai/gpt-6-luna", "policy":{
            "workspace":"project_directory", "permissions":"provider_default", "reasoning":"low"
        }
    }))
    .unwrap();
    let session_id = SessionId::new(uuid::Uuid::new_v4()).unwrap();
    let thread_id = CloudroomThreadId::try_from("thr_wire".to_owned()).unwrap();
    let prompt = LocalPrompt::try_from("exact é😀\n\t\r\"\\\u{1}".to_owned()).unwrap();
    let commands = [
        LocalCommand::Create {
            session_id,
            selection,
            prompt: prompt.clone(),
        },
        LocalCommand::Send {
            session_id,
            thread_id: thread_id.clone(),
            prompt,
        },
        LocalCommand::Inspect {
            session_id,
            thread_id: thread_id.clone(),
        },
        LocalCommand::StopThread {
            session_id,
            thread_id: thread_id.clone(),
        },
        LocalCommand::ArchiveThread {
            session_id,
            thread_id,
        },
        LocalCommand::RefreshCatalog {},
    ];
    let deadline = "2030-01-02T03:04:05.123456Z"
        .parse::<chrono::DateTime<chrono::Utc>>()
        .unwrap();
    let operations: Vec<_> = commands
        .into_iter()
        .map(|command| {
            LocalAgentOperation::queued(
                OperationId::new(uuid::Uuid::new_v4()).unwrap(),
                command,
                deadline,
            )
            .unwrap()
        })
        .collect();
    let result = node_proof(
        "verify-command-wire.mjs",
        serde_json::to_vec(&operations).unwrap(),
    )
    .await;
    assert!(result.starts_with("PASS Rust→Node command hashes: all 6 kinds"));
    println!("{}", result.trim());
}

async fn node_proof(script: &str, input: Vec<u8>) -> String {
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("integrations/cloudroom-plugin/compatibility")
        .join(script);
    let mut node = tokio::process::Command::new("node")
        .arg(script)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("Node is required for the Rust/Node wire contract proof");
    let mut stdin = node.stdin.take().unwrap();
    stdin.write_all(&input).await.unwrap();
    stdin.shutdown().await.unwrap();
    drop(stdin);
    let output = tokio::time::timeout(std::time::Duration::from_secs(10), node.wait_with_output())
        .await
        .expect("wire proof must terminate")
        .unwrap();
    assert!(
        output.status.success(),
        "Node wire proof failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[tokio::test]
async fn cloudroom_submission_rust_node_evidence_has_thread_without_turn_or_terminal_authority() {
    let operation_id = OperationId::new(uuid::Uuid::new_v4()).unwrap();
    let observations: Vec<_> = [
        SubmissionState::Accepted,
        SubmissionState::Queued,
        SubmissionState::Missing,
    ]
    .into_iter()
    .enumerate()
    .map(|(index, state)| JournalObservation::Agent {
        observation: LocalAgentObservation {
            sequence: EventSequence::try_from(index as u64 + 1).unwrap(),
            operation_id,
            evidence: LocalEvidence::Submission {
                thread_id: CloudroomThreadId::try_from("thr_known".to_owned()).unwrap(),
                state,
            },
        },
    })
    .collect();
    let result = node_proof(
        "verify-observation-wire.mjs",
        serde_json::to_vec(&observations).unwrap(),
    )
    .await;
    assert!(result.starts_with("PASS Rust→Node submission evidence"));
    println!("{}", result.trim());
}
