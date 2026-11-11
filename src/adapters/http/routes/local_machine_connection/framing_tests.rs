use super::*;
use crate::{
    application::local_agents::{ConnectionFence, MachineAuthentication, protocol::*},
    domain::local_agents::*,
};
use serde_json::{Value, json};

fn fence(generation: u64) -> ConnectionFence {
    ConnectionFence {
        machine: MachineAuthentication {
            company_id: Uuid::new_v4(),
            machine_id: MachineId::new(Uuid::new_v4()).unwrap(),
            installation_id: InstallationId::new(Uuid::new_v4()).unwrap(),
            credential_generation: CredentialGeneration::try_from(1).unwrap(),
        },
        generation: ConnectionGeneration::try_from(generation).unwrap(),
        owner_id: Uuid::new_v4(),
    }
}

#[test]
fn cloudroom_protocol_chunks_round_trip_exact_logical_limit_and_bound_each_frame() {
    let payload = json!({"data":"x".repeat(MAX_LOGICAL_BYTES-11)});
    assert_eq!(
        serde_json::to_vec(&payload).unwrap().len(),
        MAX_LOGICAL_BYTES
    );
    let frames = encode(&payload).unwrap();
    assert_eq!(frames.len(), MAX_LOGICAL_BYTES / MAX_CHUNK_BYTES);
    let mut assembler = Assembler::default();
    for frame in &frames[..frames.len() - 1] {
        assert!(frame.len() <= MAX_FRAME_BYTES);
        assert!(
            assembler.push(frame).unwrap().is_none(),
            "partial chunks cannot be acknowledged as an observation"
        );
    }
    let bytes = assembler.push(frames.last().unwrap()).unwrap().unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), payload);
    assert!(assembler.deadline().is_none());
    assert!(Assembler::handshake().push(&frames[0]).is_err());
    assert!(encode(&json!({"data":"x".repeat(MAX_LOGICAL_BYTES)})).is_err());
}

#[tokio::test(start_paused = true)]
async fn cloudroom_protocol_rejects_expired_interleaved_corrupt_and_unknown_chunks() {
    let frames = encode(&json!({"data":"x".repeat(MAX_CHUNK_BYTES)})).unwrap();
    assert_eq!(frames.len(), 2);
    let mut assembler = Assembler::default();
    assembler.push(&frames[0]).unwrap();
    tokio::time::advance(Duration::from_secs(REASSEMBLY_SECONDS)).await;
    assert!(assembler.push(&frames[1]).is_err());
    let mut assembler = Assembler::default();
    assert!(assembler.push(&frames[1]).is_err());
    assembler.push(&frames[0]).unwrap();
    let other = encode(&json!({"data":"y".repeat(MAX_CHUNK_BYTES)})).unwrap();
    assert!(assembler.push(&other[1]).is_err());
    for field in ["version", "parts", "total_bytes", "digest", "extra"] {
        let mut frame: Value = serde_json::from_str(&frames[0]).unwrap();
        frame[field] = match field {
            "version" => json!(2),
            "parts" => json!(0),
            "total_bytes" => json!(MAX_LOGICAL_BYTES + 1),
            "digest" => json!("0".repeat(64)),
            _ => json!(true),
        };
        let mut assembler = Assembler::default();
        if field == "digest" {
            assembler.push(&frame.to_string()).unwrap();
            assert!(assembler.push(&frames[1]).is_err());
        } else {
            assert!(assembler.push(&frame.to_string()).is_err());
        }
    }
    assert!(
        Assembler::default()
            .push(&"x".repeat(MAX_FRAME_BYTES + 1))
            .is_err()
    );
}

#[test]
fn cloudroom_protocol_journal_digest_excludes_current_connection_envelope() {
    let old = fence(1);
    let mut new = old;
    new.generation = ConnectionGeneration::try_from(2).unwrap();
    let observation = JournalObservation::Agent {
        observation: LocalAgentObservation {
            sequence: EventSequence::try_from(1).unwrap(),
            operation_id: OperationId::new(Uuid::new_v4()).unwrap(),
            evidence: LocalEvidence::Receipt {},
        },
    };
    let old_message = ClientMessage::Observation {
        identity: ConnectionIdentity::from_fence(old).unwrap(),
        observation: observation.clone(),
    };
    let new_message = ClientMessage::Observation {
        identity: ConnectionIdentity::from_fence(new).unwrap(),
        observation: observation.clone(),
    };
    assert_ne!(
        serde_json::to_vec(&old_message).unwrap(),
        serde_json::to_vec(&new_message).unwrap()
    );
    let ClientMessage::Observation {
        observation: decoded,
        ..
    } = serde_json::from_slice(&serde_json::to_vec(&new_message).unwrap()).unwrap()
    else {
        panic!()
    };
    assert_eq!(
        observation.canonical_digest().unwrap(),
        decoded.canonical_digest().unwrap()
    );
    let mut changed = serde_json::to_value(&observation).unwrap();
    changed["observation"]["evidence"] = json!({"kind":"uncertain"});
    let changed: JournalObservation = serde_json::from_value(changed).unwrap();
    assert_ne!(
        observation.canonical_digest().unwrap(),
        changed.canonical_digest().unwrap()
    );
}

#[test]
fn cloudroom_protocol_refuses_unsafe_javascript_integers_and_nul_output() {
    assert!(ConnectionIdentity::from_fence(fence(MAX_SAFE_INTEGER + 1)).is_err());
    let current = fence(1);
    let hello = JournalHello {
        version: 2,
        installation_id: current.machine.installation_id,
        journal_id: Uuid::new_v4(),
        journal_sequence: MAX_SAFE_INTEGER + 1,
    };
    assert!(hello.validate(current).is_err());
    assert!(LocalOutput::try_from("invalid\0output".to_owned()).is_err());
    assert!(LocalOutput::try_from("x".repeat(MAX_RESULT_BYTES)).is_ok());
    assert!(LocalOutput::try_from("x".repeat(MAX_RESULT_BYTES + 1)).is_err());
    let mut extra = serde_json::to_value(ClientMessage::Heartbeat {
        identity: ConnectionIdentity::from_fence(current).unwrap(),
    })
    .unwrap();
    extra["company_id"] = json!(Uuid::new_v4());
    assert!(serde_json::from_value::<ClientMessage>(extra).is_err());
}

#[test]
fn cloudroom_protocol_two_rejects_old_hello_and_retains_chunk_framing_one() {
    assert_eq!(
        crate::application::local_agents::bounds::PROTOCOL_VERSION,
        2
    );
    assert_eq!(crate::application::local_agents::bounds::FRAME_VERSION, 1);
    let current = fence(1);
    let mut hello = JournalHello {
        version: 2,
        installation_id: current.machine.installation_id,
        journal_id: Uuid::new_v4(),
        journal_sequence: 0,
    };
    hello.validate(current).unwrap();
    hello.version = 1;
    assert!(hello.validate(current).is_err());
}
