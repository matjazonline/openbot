//! Manual starts and reruns (p08 D1.1, WF-C001): a published workflow's active binding, started
//! with the caller's own authority under a client idempotency key, returning the run identity at
//! once. A rerun names the top-level run it reruns; admission checks the original.

use super::message_admission::WorkflowAdmitter;
use super::{
    AdmissionResult, AdmitWorkflowRequest, DeliveryPolicy, IdempotencyKey, RelatedAssociation,
    WorkflowActor,
};
use crate::application::app_error::{AppError, AppResult};
use crate::domain::entities::correlation::CorrelationId;
use crate::domain::workflow::{
    CompanyId, RunId, TriggerId, TriggerRef, TriggerSource, WorkflowBindingId,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Re-reads of a binding reconfigured between preparation and commit.
const MAX_STALE_RETRIES: usize = 3;

pub struct StartRun {
    pub company_id: CompanyId,
    pub actor: WorkflowActor,
    pub binding_id: WorkflowBindingId,
    /// Within the binding's association; a run of a company binding may name a channel or thread.
    pub association: RelatedAssociation,
    pub input: Value,
    /// The client's key, at most [`MAX_CLIENT_KEY_BYTES`]. Admission sees it as `u:{key}`, a
    /// namespace no automatic trigger uses (`m:` messages, `s:` schedule occurrences), so a
    /// client key can never claim an automatic admission's key.
    pub idempotency_key: String,
    pub rerun_of: Option<RunId>,
}

pub const MAX_CLIENT_KEY_BYTES: usize = super::MAX_IDEMPOTENCY_KEY_BYTES - 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartedRun {
    pub run: RunId,
    /// The key was used before for this same start; this is its run.
    pub replayed: bool,
}

/// Start a run. Reusing a key with a different binding, association, input or rerun original is
/// a conflict, never a second run.
pub async fn start(
    admitter: &(impl WorkflowAdmitter + ?Sized),
    command: StartRun,
) -> AppResult<StartedRun> {
    let key = manual_key(&command.idempotency_key)?;
    for _ in 0..MAX_STALE_RETRIES {
        let result = admitter
            .admit(AdmitWorkflowRequest {
                company_id: command.company_id,
                actor: command.actor,
                association: command.association,
                trigger: TriggerRef::new(
                    command.company_id,
                    manual_trigger(command.company_id, &key),
                    TriggerSource::Manual,
                )
                .map_err(|error| AppError::Internal(error.to_string()))?,
                correlation_id: CorrelationId::new(),
                binding_id: command.binding_id,
                idempotency_key: key.clone(),
                input: command.input.clone(),
                rerun_of: command.rerun_of,
                delivery_policy: DeliveryPolicy::Deliver,
                expected_binding_revision: None,
                admission_claim: None,
            })
            .await?;
        match result {
            AdmissionResult::Created(run) => {
                return Ok(StartedRun {
                    run,
                    replayed: false,
                });
            }
            AdmissionResult::Replayed(run) => {
                return Ok(StartedRun {
                    run,
                    replayed: true,
                });
            }
            AdmissionResult::Conflict => {
                return Err(AppError::Conflict(
                    "This idempotency key already started a different workflow run".into(),
                ));
            }
            // Admission re-prepares against the current configuration on the next attempt.
            AdmissionResult::Stale => {}
        }
    }
    Err(AppError::Conflict(
        "The workflow binding kept changing; try again".into(),
    ))
}

pub fn manual_key(client_key: &str) -> AppResult<IdempotencyKey> {
    if client_key.is_empty() || client_key.len() > MAX_CLIENT_KEY_BYTES {
        return Err(AppError::BadRequest(
            "invalid workflow idempotency key".into(),
        ));
    }
    IdempotencyKey::parse(format!("u:{client_key}"))
}

/// A manual start's trigger is named by its key, so repeating the key names the same trigger
/// and a different key never aliases it (the source key of a manual admission is its trigger).
pub fn manual_trigger(company: CompanyId, key: &IdempotencyKey) -> TriggerId {
    let mut digest = Sha256::new();
    digest.update(b"workflow.manual\0");
    digest.update(company.as_uuid().as_bytes());
    digest.update(key.as_str().as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest.finalize()[..16]);
    // RFC 9562 version 8 (custom), variant 10.
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    TriggerId::new(Uuid::from_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_trigger_is_stable_per_company_and_key() {
        let company = CompanyId::new(Uuid::new_v4());
        let other = CompanyId::new(Uuid::new_v4());
        let key = manual_key("start-1").unwrap();
        let next = manual_key("start-2").unwrap();
        assert_eq!(manual_trigger(company, &key), manual_trigger(company, &key));
        assert_ne!(
            manual_trigger(company, &key),
            manual_trigger(company, &next)
        );
        assert_ne!(manual_trigger(company, &key), manual_trigger(other, &key));
        let id = manual_trigger(company, &key).as_uuid();
        assert_eq!(id.get_version_num(), 8);
    }

    #[test]
    fn manual_keys_have_their_own_namespace() {
        assert_eq!(manual_key("s:1").unwrap().as_str(), "u:s:1");
        assert!(manual_key("").is_err());
        assert!(manual_key(&"k".repeat(MAX_CLIENT_KEY_BYTES)).is_ok());
        assert!(manual_key(&"k".repeat(MAX_CLIENT_KEY_BYTES + 1)).is_err());
        assert!(manual_key("no spaces").is_err());
    }
}
