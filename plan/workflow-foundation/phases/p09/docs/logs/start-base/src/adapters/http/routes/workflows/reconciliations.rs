//! Action reconciliation as an operator annotation (P08-D3 rev 2): the route over the existing
//! reconciliation command, composed with the command actor's current access to the action's
//! resource and no evidence verifier, so only an unknown note settles and nothing resumes.
//!
//! Outcomes are results: the command records a refusal under its key. 200 answers a recorded
//! outcome; 409 answers `revision_conflict`, `idempotency_conflict`, `blocked` and
//! `evidence_conflict` (the note is recorded and conflicts with other evidence). After a 409 the
//! caller re-reads the run and sends a new key, because the request digest includes the revision
//! and the evidence. After a 503 the settlement may already have committed, so the caller retries
//! the same request under the same key, which replays a recorded outcome. Every call, a replay
//! included, needs current access to the run and a ready resource; otherwise it is 404 and
//! nothing is recorded (§D3.2a).
use super::*;
use crate::application::workflow::actions::{
    ActionScope, ApprovalSubject, ArgumentDigest, EvidenceInput, ReconcileActionCommand,
    ReconciliationOutcome, ReconciliationTarget,
};
use crate::application::workflow::{IdempotencyKey, RunRevision};
use crate::domain::workflow::{ActionInvocationId, ActionRemoteMarkerId, ExecutionId, RunId};
use axum::extract::{Path, State};
use serde::Deserialize;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// The command's whole budget; the service caps it at 5 s as well.
const BUDGET: Duration = Duration::from_secs(5);

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TargetBody {
    Remote,
    MessageDelivery { ordinal: u8, delivery_id: Uuid },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReconcileBody {
    pub execution_id: Uuid,
    pub invocation_id: Uuid,
    pub argument_digest: ArgumentDigest,
    pub marker_id: Uuid,
    pub target: TargetBody,
    pub expected_revision: u64,
    pub command_key: String,
    /// The command's own evidence shape; without a verifier only `unknown_note` settles.
    pub evidence: EvidenceInput,
}

fn status(outcome: &ReconciliationOutcome) -> StatusCode {
    match outcome {
        ReconciliationOutcome::UnknownRecorded
        | ReconciliationOutcome::AppliedRecorded { .. }
        | ReconciliationOutcome::NotAppliedRecorded
        | ReconciliationOutcome::Scheduled { .. }
        | ReconciliationOutcome::DeliveryReopened => StatusCode::OK,
        ReconciliationOutcome::RevisionConflict
        | ReconciliationOutcome::IdempotencyConflict
        | ReconciliationOutcome::Blocked { .. }
        | ReconciliationOutcome::EvidenceConflict => StatusCode::CONFLICT,
    }
}

pub async fn reconcile(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path((company_id, run)): Path<(Uuid, Uuid)>,
    Json(body): Json<ReconcileBody>,
) -> WorkflowResult<(StatusCode, Json<Value>)> {
    let command = ReconcileActionCommand {
        actor: actor(&user)?,
        scope: ActionScope {
            company: company(company_id),
            run: RunId::new(run),
            execution: ExecutionId::new(body.execution_id),
        },
        subject: ApprovalSubject {
            invocation: ActionInvocationId::new(body.invocation_id),
            argument_digest: body.argument_digest,
        },
        marker: ActionRemoteMarkerId::new(body.marker_id),
        target: match body.target {
            TargetBody::Remote => ReconciliationTarget::Remote,
            TargetBody::MessageDelivery {
                ordinal,
                delivery_id,
            } => ReconciliationTarget::MessageDelivery {
                ordinal,
                delivery: delivery_id,
            },
        },
        expected_revision: RunRevision(body.expected_revision),
        command_key: IdempotencyKey::parse(&body.command_key)?,
        input: body.evidence,
    };
    // Boxed: the command's two transactions and its resource checks stay off this frame.
    let result = Box::pin(engine.reconciliation().reconcile(
        &command,
        &CancellationToken::new(),
        BUDGET,
    ))
    .await?;
    Ok((
        status(&result.outcome),
        ok(json!({
            "outcome": result.outcome,
            "revision": result.revision.0,
            "evidence_id": result.evidence.map(|evidence| evidence.as_uuid()),
            "replayed": result.replayed,
        })),
    ))
}
