//! Human decisions (05-D1 amended by D2–D4): an explicit list of company-member reviewers,
//! append-only comments separate from the one submission, and durable member notices on the
//! delivery engine. The wait row owns the lifecycle; the decision row owns the frozen
//! assignment and the single authoritative submission.
use super::{
    activation::*,
    authorization::WorkflowActor,
    batch::{PureCompletion, validate_completion},
    publication::PublishedBundle,
    waits::ParkedWorkflow,
};
use crate::app_error::{AppError, AppResult};
use crate::domain::workflow::{
    ChoiceName, CompanyId, ExecutionId, RouteSelection, RunId, StepId, validate_context_value,
};
use crate::entities::transport::DeliveryPurpose;
use crate::entities::value_objects::EmailAddress;
use crate::transport::{
    CanonicalContent, DeliveryComposer, DeliveryContext, EmailDeliveryContext, EmailThreading,
    NewStandaloneDelivery, StandaloneDeliveryRequest,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use uuid::Uuid;

/// The comment bound per decision (05-D2 §12), also enforced by the schema.
pub const MAX_DECISION_COMMENTS: i64 = 1000;
/// The comment body bound in characters (05-D2 §12), also enforced by the schema.
pub const MAX_COMMENT_CHARS: usize = 16_384;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DecisionId(pub Uuid);

/// A client-chosen idempotency key: 1..128 printable ASCII bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionCommandKey(String);
impl DecisionCommandKey {
    pub fn parse(value: impl Into<String>) -> AppResult<Self> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 128
            || !value.bytes().all(|byte| (33..=126).contains(&byte))
        {
            return Err(AppError::BadRequest("Invalid decision command key".into()));
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The final choice, feedback and data, against the revision and artifact the reviewer saw.
#[derive(Debug, Clone, PartialEq)]
pub struct SubmitDecision {
    pub company: CompanyId,
    pub actor: WorkflowActor,
    pub decision: DecisionId,
    pub command_key: DecisionCommandKey,
    pub expected_revision: i64,
    pub expected_artifact: String,
    pub choice: String,
    pub feedback: String,
    pub data: Value,
}
impl SubmitDecision {
    /// Replay identity: the same key with a different command is a conflict.
    pub(crate) fn fingerprint(&self) -> AppResult<String> {
        if self.choice.len() > 140
            || self.feedback.len() > 65_536
            || self.expected_artifact.len() != 64
        {
            return Err(AppError::BadRequest("Invalid decision submission".into()));
        }
        validate_context_value(
            &self.data,
            crate::domain::workflow::ContextLimits::default(),
        )
        .map_err(|_| AppError::BadRequest("Decision data exceeds context limits".into()))?;
        hex_digest(&json!({
            "operation":"submit",
            "decision":self.decision.0,
            "expected_revision":self.expected_revision,
            "expected_artifact":self.expected_artifact,
            "choice":self.choice,
            "feedback":self.feedback,
            "data":self.data,
        }))
    }
}

/// Append-only discussion; never settles the decision or changes its revision.
#[derive(Debug, Clone, PartialEq)]
pub struct AddDecisionComment {
    pub company: CompanyId,
    pub actor: WorkflowActor,
    pub decision: DecisionId,
    pub command_key: DecisionCommandKey,
    pub body: String,
}
impl AddDecisionComment {
    pub(crate) fn validate(&self) -> AppResult<()> {
        let length = self.body.chars().count();
        if length == 0 || length > MAX_COMMENT_CHARS {
            return Err(AppError::BadRequest("Invalid decision comment".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionDisposition {
    Applied,
    Replayed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecisionSubmission {
    pub disposition: DecisionDisposition,
    pub revision: i64,
    /// The continuation committed with the submission; `None` when the route ends the run or
    /// the run's activation bound failed it.
    pub successor: Option<ActivationRequest>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecisionComment {
    pub disposition: DecisionDisposition,
    pub sequence: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecisionCommentView {
    pub sequence: i64,
    pub author_principal: Uuid,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecisionSubmissionView {
    pub submitted_by: Uuid,
    pub submitted_at: DateTime<Utc>,
    pub choice: String,
    pub feedback: String,
    pub data: Value,
}

/// What a member with run visibility may see. Comments are internal discussion.
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionView {
    pub decision: DecisionId,
    pub run: RunId,
    pub execution: ExecutionId,
    pub step: StepId,
    pub revision: i64,
    pub artifact_digest: String,
    pub proposal: Value,
    pub deadline: DateTime<Utc>,
    /// The wait state: `waiting` is open; any other value is settled.
    pub state: String,
    pub reviewers: Vec<Uuid>,
    pub submission: Option<DecisionSubmissionView>,
    pub comments: Vec<DecisionCommentView>,
}

#[async_trait]
pub trait WorkflowDecisions: Send + Sync {
    /// Park a `decision.human` step: wait, decision, reviewers and one notice per reviewer in
    /// one transaction. A configuration failure fails the run unrouted; nothing parks.
    async fn park_decision(
        &self,
        scope: ActivationRequest,
        notices: &DecisionNotices,
    ) -> AppResult<Option<ParkedWorkflow>>;
    /// The first valid authorized submission settles the decision and commits its continuation.
    async fn submit_decision(&self, command: SubmitDecision) -> AppResult<DecisionSubmission>;
    async fn add_decision_comment(&self, command: AddDecisionComment)
    -> AppResult<DecisionComment>;
    async fn read_decision(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        decision: DecisionId,
    ) -> AppResult<DecisionView>;
}

/// The frozen assignment facts of one decision execution.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DecisionSpecification {
    /// User ids, in declared order.
    pub reviewers: Vec<Uuid>,
    pub deadline: DateTime<Utc>,
    pub proposal_digest: String,
}

pub(crate) fn specification(
    bundle: &PublishedBundle,
    activation: &ActivatedExecution,
) -> AppResult<DecisionSpecification> {
    let step = bundle
        .compiled()
        .graph()
        .definition()
        .steps
        .get(&activation.step)
        .ok_or_else(invalid)?;
    if step.step_type.as_str() != "decision.human" {
        return Err(invalid());
    }
    let (reviewers, deadline) = reviewer_deadline(&activation.inputs)?;
    let proposal = activation.inputs.get("proposal").ok_or_else(invalid)?;
    Ok(DecisionSpecification {
        reviewers,
        deadline,
        proposal_digest: hex_digest(proposal)?,
    })
}

/// Validate a submission into the step's completion without settling anything. The choice must
/// be declared; `{choice, data, feedback}` must satisfy the step's output contract (data
/// schema, feedback rules and lengths) and the run's context limits (05-D2 §7.2).
pub(crate) fn reviewer_deadline(inputs: &Value) -> AppResult<(Vec<Uuid>, DateTime<Utc>)> {
    let deadline = inputs
        .get("deadline")
        .and_then(Value::as_str)
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .ok_or_else(invalid)?
        .with_timezone(&Utc);
    let items = inputs
        .get("reviewers")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    if items.is_empty() || items.len() > super::registry::MAX_REVIEWERS {
        return Err(invalid());
    }
    let mut reviewers = Vec::with_capacity(items.len());
    for item in items {
        if item.get("kind").and_then(Value::as_str) != Some("user") {
            return Err(invalid());
        }
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .and_then(|text| Uuid::parse_str(text).ok())
            .ok_or_else(invalid)?;
        if reviewers.contains(&id) {
            return Err(invalid());
        }
        reviewers.push(id);
    }
    Ok((reviewers, deadline))
}

pub(crate) fn prepare_submission(
    bundle: &PublishedBundle,
    activation: &ActivatedExecution,
    command: &SubmitDecision,
) -> AppResult<PureCompletion> {
    let choice = ChoiceName::parse(&command.choice)
        .map_err(|_| AppError::BadRequest("Unknown decision choice".into()))?;
    let output = json!({"choice":command.choice,"data":command.data,"feedback":command.feedback});
    validate_context_value(&output, bundle.compiled().graph().context_limits())
        .map_err(|_| AppError::BadRequest("Decision submission exceeds context limits".into()))?;
    validate_completion(bundle, activation, output, RouteSelection::Choice(choice))
}

/// Canonical SHA-256: object keys sorted at every depth, so a digest names content, not order.
pub(crate) fn hex_digest(value: &Value) -> AppResult<String> {
    let value = super::canonical::context(value)?;
    super::canonical::digest(&value)
}

/// One reviewer's notice, as the park transaction resolved it.
#[derive(Debug, Clone)]
pub(crate) struct DecisionNotice<'a> {
    pub decision: Uuid,
    pub principal: Uuid,
    pub recipient: EmailAddress,
    pub correlation: Uuid,
    pub workflow_title: &'a str,
    pub step: &'a StepId,
    pub company_label: &'a str,
    pub deadline: DateTime<Utc>,
}

/// Renders member notices for the existing delivery engine (05-D2 §10). Pure: no I/O, so the
/// park transaction can call it. The link is a page reference, never a credential.
#[derive(Clone)]
pub struct DecisionNotices {
    deliveries: DeliveryComposer,
    public_base_url: String,
    from: String,
    subject_prefix: String,
}
impl DecisionNotices {
    /// `public_base_url` is `AppConfig::public_base_url()`; `app_domain` is `APP_DOMAIN_NAME`.
    pub fn new(deliveries: DeliveryComposer, public_base_url: &str, app_domain: &str) -> Self {
        Self {
            deliveries,
            public_base_url: public_base_url.trim_end_matches('/').to_string(),
            from: format!("notifications@{app_domain}"),
            subject_prefix: "Review requested".into(),
        }
    }

    /// A composer whose subject cannot render, for the deterministic-failure path.
    #[cfg(test)]
    pub(crate) fn unrenderable(mut self) -> Self {
        self.subject_prefix = "x".repeat(4096);
        self
    }

    /// The prefix the reviewer row's provenance trigger requires (05-D3 §3.1).
    pub(crate) fn expected_key_prefix(decision: Uuid, principal: Uuid) -> String {
        format!("notification:workflow-decision:{decision}:principal:{principal}:")
    }

    /// A deterministic failure (unrenderable content, or a key the trigger cannot prove because
    /// a long address made `delivery_key` hash it) is `BadRequest`: the reviewer cannot be given a
    /// provable notice, and the park settles `decision.reviewer_unavailable` (05-D4 §2).
    pub(crate) fn compose(&self, notice: &DecisionNotice<'_>) -> AppResult<NewStandaloneDelivery> {
        let subject = format!(
            "{}: {} / {}",
            self.subject_prefix, notice.workflow_title, notice.step
        );
        let body = format!(
            "A workflow decision is waiting for your review in {}.\n\nDeadline: {}\nDecision: {}\n\nOpen it: {}/ui/workflow-decisions/{}",
            notice.company_label,
            notice.deadline.to_rfc3339(),
            notice.decision,
            self.public_base_url,
            notice.decision,
        );
        let content = CanonicalContent::parse(subject, body)
            .map_err(|_| AppError::BadRequest("Decision notice cannot be rendered".into()))?;
        let delivery = self
            .deliveries
            .compose_standalone(StandaloneDeliveryRequest {
                correlation_id: notice.correlation.into(),
                purpose: DeliveryPurpose::Notification,
                source_key: format!(
                    "workflow-decision:{}:principal:{}",
                    notice.decision, notice.principal
                ),
                content: &content,
                context: DeliveryContext::Email(EmailDeliveryContext {
                    from: self.from.clone().into(),
                    from_name: Some("Mail Agents".into()),
                    recipient_to: notice.recipient.clone(),
                    recipients_cc: Vec::new(),
                    threading: EmailThreading::Standalone,
                    relay: None,
                }),
            })?;
        if !delivery
            .idempotency_key
            .as_str()
            .starts_with(&Self::expected_key_prefix(
                notice.decision,
                notice.principal,
            ))
        {
            return Err(AppError::BadRequest(
                "Decision notice key cannot be proven".into(),
            ));
        }
        Ok(delivery)
    }
}

fn invalid() -> AppError {
    AppError::BadRequest("Invalid workflow decision".into())
}
