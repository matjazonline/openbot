//! Inbound ingest: one canonical message, one atomic commit.
//!
//! The pipeline is named phases, in this order:
//!
//! 1. [`policy::guard_ingress`] -- pure rejections: authentication, hops, loops, auto-replies.
//! 2. [`ThreadUseCases::resolve_addresses`] -- selectors to tenant-scoped channels and bindings,
//!    with the principal/ACL decision folded into the same walk over the addresses.
//! 3. [`ThreadUseCases::prepare_channels`] -- the thread each channel continues or opens, and the
//!    pure participant, third-party and turn-limit policy that goes with it.
//! 4. [`commit::CommitPlan`] -- one `InboundCommitRequest` naming every row that must agree,
//!    handed to the committer in a single call.
//! 5. [`ThreadUseCases::assemble_result`] -- read back what was committed, for the caller that is
//!    about to run the agent in-process.
//!
//! Nothing before the commit writes anything. An authorization or validation rejection therefore
//! leaves no message, no mapping, no thread and no task behind, which is the property the previous
//! shape -- a `create_thread` here, a `create_message` there, an `enqueue_task` at the end -- could
//! not state.
//!
//! The I/O phases stay `async` and the decisions do not: everything in [`policy`] is a free
//! function over already-loaded values, which is what lets those rules be unit-tested with no
//! database and no mocks, and what keeps this chain's stack frames shallow.

pub(crate) mod commit;
pub(crate) mod policy;
mod routing;

use std::{
    sync::Arc,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use tracing::{info, instrument, warn};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        correlation::CorrelationId,
        message::{AttachmentMetadata, CanonicalMessageId},
        transport::QualifiedIdentity,
        value_objects::ThreadIndexParseError,
    },
    transport::{
        BoundedVec, InboundCommitOutcome, InboundCommitRequest, InboundDraft, InboundRouting,
        MAX_ATTACHMENTS, MessageDisposition, ReplyDelivery,
    },
    use_cases::thread::{
        ChannelMatch, InboundIngestResult, ThreadUseCases, ingest::commit::CommitPlan,
        support::DirectoryCache,
    },
};

pub use policy::{IngestRejection, IngressOrigin};

/// The one task type this pipeline produces.
pub(super) const AGENT_DISPATCH_TASK: &str = "email_agent_dispatch";

/// A protocol-agnostic, canonical message ingress intent.
///
/// Direct application entry points (UI reply/compose, schedulers, internal tools,
/// non-email transports) construct this to submit a message to a channel without
/// synthesizing or parsing MIME/RFC email strings and headers.
#[derive(Debug, Clone)]
pub struct CanonicalMessageIngress {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    /// Explicit target thread, if continuing an existing conversation.
    pub target_thread_id: Option<Uuid>,
    /// Specific message turn being answered, if this is a direct reply.
    pub reply_to_message_id: Option<CanonicalMessageId>,
    /// The qualified identity of the author (e.g. email, user, or platform).
    pub author: QualifiedIdentity,
    /// Optional conversation/thread subject. If `None` on a reply, inherits from the thread.
    pub subject: Option<String>,
    /// The text body of the message.
    pub text_body: String,
    /// Canonical attachment metadata already stored or associated with the message.
    pub attachments: Vec<AttachmentMetadata>,
    /// Whether the response from an agent should be delivered externally or kept in-app.
    pub reply_delivery: ReplyDelivery,
    /// Whether to run an agent (`Answer`) or record context only (`FileOnly`).
    pub disposition: MessageDisposition,
    /// The authenticated origin of the request.
    pub origin: IngressOrigin,
    /// Optional correlation id, preserved if this is part of an existing chain.
    pub correlation_id: Option<CorrelationId>,
}

impl CanonicalMessageIngress {
    pub fn new(
        company_id: Uuid,
        channel_id: Uuid,
        author: QualifiedIdentity,
        text_body: impl Into<String>,
        origin: IngressOrigin,
    ) -> Self {
        Self {
            company_id,
            channel_id,
            target_thread_id: None,
            reply_to_message_id: None,
            author,
            subject: None,
            text_body: text_body.into(),
            attachments: Vec::new(),
            reply_delivery: ReplyDelivery::Send,
            disposition: MessageDisposition::Answer,
            origin,
            correlation_id: None,
        }
    }

    pub fn with_target_thread(mut self, thread_id: Uuid) -> Self {
        self.target_thread_id = Some(thread_id);
        self
    }

    pub fn with_reply_to_message(mut self, message_id: CanonicalMessageId) -> Self {
        self.reply_to_message_id = Some(message_id);
        self
    }

    pub fn with_subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    pub fn with_attachments(mut self, attachments: Vec<AttachmentMetadata>) -> Self {
        self.attachments = attachments;
        self
    }

    pub fn with_reply_delivery(mut self, delivery: ReplyDelivery) -> Self {
        self.reply_delivery = delivery;
        self
    }

    pub fn with_disposition(mut self, disposition: MessageDisposition) -> Self {
        self.disposition = disposition;
        self
    }

    pub fn with_correlation_id(mut self, correlation_id: CorrelationId) -> Self {
        self.correlation_id = Some(correlation_id);
        self
    }
}

/// One inbound message offered to the application, whatever transport carried it.
///
/// The adapter states the first two fields; only the code path that actually authenticated the
/// message can state `origin`, which is what keeps a header from claiming to be a trusted internal
/// relay.
#[derive(Debug, Clone)]
pub struct InboundMessage {
    pub draft: InboundDraft,
    pub routing: InboundRouting,
    pub origin: IngressOrigin,
    pub reply_delivery: ReplyDelivery,
    /// Hints the adapter parsed but could not use, for the counters that watch them.
    pub unusable_hints: Vec<UnusableHint>,
}

/// The result of every read-only ingress decision. Only the accepted arm may proceed to uploads
/// and the atomic commit.
pub enum InboundPreflight {
    Rejected(Box<InboundIngestResult>),
    Accepted(Box<PreparedInbound>),
}

/// An accepted, still-uncommitted message. Attachment bytes remain outside this type; the email
/// adapter replaces their metadata after content-addressed storage succeeds or fails.
pub struct PreparedInbound {
    plan: CommitPlan,
    stored_attachment_count: usize,
    failed_attachment_count: usize,
}

impl PreparedInbound {
    /// This message as the commit request it will be written by, without writing it.
    ///
    /// The two ingress shapes need the same plan at different moments. A listener that must answer
    /// its own session commits inline through [`ThreadUseCases::commit_prepared_inbound`] and reads
    /// the result back. A transport whose events come off the durable inbox cannot: the worker owns
    /// the execution fence, and only it may put the live lease into `claimed_event` so the event's
    /// completion and the message become durable together. So a decoder hands the plan over and the
    /// worker commits it.
    ///
    /// `claimed_event` is deliberately left `None` here. Filling it is the worker's alone.
    pub fn into_commit_request(self) -> InboundCommitRequest {
        self.plan.request()
    }

    pub fn replace_attachments(
        &mut self,
        attachments: BoundedVec<crate::entities::message::AttachmentMetadata, MAX_ATTACHMENTS>,
        stored_count: usize,
        failed_count: usize,
    ) {
        self.plan.replace_attachments(attachments);
        self.stored_attachment_count = stored_count;
        self.failed_attachment_count = failed_count;
    }
}

impl InboundMessage {
    /// A message that arrived over a verifying transport and expects a real answer.
    pub fn arriving(draft: InboundDraft, routing: InboundRouting, origin: IngressOrigin) -> Self {
        Self {
            draft,
            routing,
            origin,
            reply_delivery: ReplyDelivery::Send,
            unusable_hints: Vec::new(),
        }
    }

    pub fn with_unusable_hints(mut self, hints: Vec<UnusableHint>) -> Self {
        self.unusable_hints = hints;
        self
    }

    pub fn with_reply_delivery(mut self, reply_delivery: ReplyDelivery) -> Self {
        self.reply_delivery = reply_delivery;
        self
    }
}

impl ThreadUseCases {
    /// Take one inbound message all the way from "an adapter parsed it" to "it is durable".
    #[instrument(skip(self, message), fields(origin = ?message.origin))]
    pub async fn ingest(&self, message: InboundMessage) -> AppResult<InboundIngestResult> {
        // Box the two-phase seam so callers without attachment bytes keep this already-deep
        // ingress future small, while SMTP and webhooks can pause between policy and persistence.
        match Box::pin(self.preflight_inbound(message)).await? {
            InboundPreflight::Rejected(result) => Ok(*result),
            InboundPreflight::Accepted(prepared) => {
                Box::pin(self.commit_prepared_inbound(*prepared)).await
            }
        }
    }

    /// Run guards, routing, ACLs, thread policy and bounds without writing domain rows or objects.
    pub async fn preflight_inbound(&self, message: InboundMessage) -> AppResult<InboundPreflight> {
        let InboundMessage {
            draft,
            routing,
            origin,
            reply_delivery,
            unusable_hints,
        } = message;

        self.record_unusable_hints(&unusable_hints);
        if let Err(rejection) = policy::guard_ingress(&draft, origin) {
            warn!(%rejection, "Refusing an inbound message at the ingress guard");
            return Ok(InboundPreflight::Rejected(Box::new(
                InboundIngestResult::rejected(rejection),
            )));
        }

        let mut directory = DirectoryCache::new(self);

        // Answered before routing, so a `_help` copied onto a real message still gets its reply and
        // a message that named nothing else reports the answer rather than an unknown address.
        let answered_system = self
            .answer_system_addresses(&draft, &routing, origin, &mut directory)
            .await?;

        let resolved = match self
            .resolve_addresses(&draft, &routing, origin, &mut directory)
            .await?
        {
            Ok(resolved) => resolved,
            Err(IngestRejection::UnknownRecipient) if answered_system => {
                return Ok(InboundPreflight::Rejected(Box::new(
                    InboundIngestResult::rejected(IngestRejection::SystemAddressAnswered),
                )));
            }
            Err(rejection) => {
                return Ok(InboundPreflight::Rejected(Box::new(
                    InboundIngestResult::rejected(rejection),
                )));
            }
        };

        let prepared = match self
            .prepare_channels(&draft, &routing, &resolved, &mut directory)
            .await?
        {
            Ok(prepared) => prepared,
            Err(rejection) => {
                return Ok(InboundPreflight::Rejected(Box::new(
                    InboundIngestResult::rejected(rejection),
                )));
            }
        };

        let plan = CommitPlan::build(&draft, &resolved, prepared, reply_delivery)?;
        Ok(InboundPreflight::Accepted(Box::new(PreparedInbound {
            plan,
            stored_attachment_count: 0,
            failed_attachment_count: 0,
        })))
    }

    /// Persist a preflighted message. Object-storage failures are already reflected in its
    /// metadata; a database failure records successfully uploaded objects as reconciliation
    /// candidates because no message row can reference them.
    pub async fn commit_prepared_inbound(
        &self,
        prepared: PreparedInbound,
    ) -> AppResult<InboundIngestResult> {
        let PreparedInbound {
            plan,
            stored_attachment_count,
            failed_attachment_count,
        } = prepared;
        if failed_attachment_count > 0
            && let Some(monitoring) = self.monitoring.as_ref()
        {
            monitoring.increment_counter(
                "inbound_attachment_storage_failures_total",
                failed_attachment_count as u64,
                &[],
            );
        }
        info!(
            company_id = %plan.company_id(),
            channels = plan.channels(),
            disposition = ?plan.disposition(),
            "Committing an inbound message"
        );

        let outcome = match self.committer.commit_inbound(plan.request()).await {
            Ok(outcome) => outcome,
            Err(error) => {
                if stored_attachment_count > 0
                    && let Some(monitoring) = self.monitoring.as_ref()
                {
                    monitoring.increment_counter(
                        "inbound_attachment_orphan_candidates_total",
                        stored_attachment_count as u64,
                        &[],
                    );
                }
                return Err(error);
            }
        };
        self.assemble_result(plan, outcome).await
    }

    /// Count the conversation hints an adapter parsed but could not use.
    ///
    /// Parsing belongs to the adapter and metrics belong here, so the adapter reports the failure
    /// as data instead of reaching for a monitoring handle it has no business holding. The warning
    /// is rate-limited per reason because a single misbehaving client can produce thousands a
    /// minute and the useful signal is "this is still happening", not every instance.
    fn record_unusable_hints(&self, hints: &[UnusableHint]) {
        for hint in hints {
            let UnusableHint::ThreadIndex(error, encoded_length) = hint;
            let reason = error.metric_reason();
            if let Some(monitoring) = self.monitoring.as_ref() {
                monitoring.increment_counter(
                    "thread_index_rejected_total",
                    1,
                    &[("reason", reason)],
                );
                monitoring.record_gauge(
                    "thread_index_rejected_encoded_bytes",
                    *encoded_length as f64,
                    &[("reason", reason)],
                );
            }
            if should_warn_about_thread_index(*error) {
                warn!(
                    target: "mail_agents::thread_index",
                    reason,
                    encoded_length,
                    "Ignoring a malformed Thread-Index header"
                );
            }
        }
    }

    /// Phase 6: read back what the commit made durable.
    ///
    /// The commit returns identifiers only, so the threads and the stored message are loaded here
    /// rather than handed back through the port. That keeps the transaction boundary free of
    /// read-model shapes, and it means the caller about to run the agent in-process is looking at
    /// rows that are actually committed rather than at values it hoped were written.
    async fn assemble_result(
        &self,
        plan: CommitPlan,
        outcome: InboundCommitOutcome,
    ) -> AppResult<InboundIngestResult> {
        let envelope = Arc::new(plan.envelope().clone());
        let disposition = plan.disposition();
        let reply_delivery = plan.reply_delivery();
        let prepared = plan.into_prepared();

        let mut channel_matches = Vec::with_capacity(prepared.channels.len());
        for (prepared_channel, thread_id) in prepared.channels.iter().zip(&outcome.thread_ids) {
            let thread = self
                .thread_persistence
                .get_thread_by_id(*thread_id)
                .await?
                .ok_or_else(|| {
                    AppError::Internal(format!("Thread {thread_id} vanished after its own commit"))
                })?;
            // The commit returns thread ids in association order, so the pairing above is
            // positional. Checked rather than trusted: a mispaired thread would attribute one
            // channel's conversation to another, and the redelivery path builds its list from the
            // associations the *first* delivery wrote rather than from this request's.
            if thread.channel_id != prepared_channel.candidate.channel.id {
                return Err(AppError::Internal(format!(
                    "Committed thread {thread_id} belongs to channel {} rather than {}",
                    thread.channel_id, prepared_channel.candidate.channel.id
                )));
            }
            let inbound_message = self
                .thread_persistence
                .get_thread_message(*thread_id, outcome.message_id)
                .await?
                .ok_or_else(|| {
                    AppError::Internal(format!(
                        "Message {} vanished after its own commit",
                        outcome.message_id
                    ))
                })?;
            channel_matches.push(ChannelMatch {
                company: prepared_channel.candidate.company.clone(),
                channel: prepared_channel.candidate.channel.clone(),
                matched_slug: Some(prepared_channel.candidate.matched_slug.clone()),
                thread,
                inbound_message,
                recipient_role: prepared_channel.candidate.role,
                step: prepared_channel.candidate.step,
            });
        }

        let primary = channel_matches
            .first()
            .cloned()
            .ok_or_else(|| AppError::Internal("An accepted message reached no thread".into()))?;
        if !disposition.answers() {
            info!(
                message_id = %outcome.message_id,
                thread_id = %primary.thread.id,
                "Filed an inbound message without running an agent"
            );
        }

        Ok(InboundIngestResult {
            accepted: true,
            rejection: None,
            thread: Some(primary.thread.clone()),
            inbound_message: Some(primary.inbound_message.clone()),
            company: Some(primary.company.clone()),
            channel: Some(primary.channel.clone()),
            envelope: Some(envelope),
            task_id: outcome.task_id,
            reply_delivery,
            channel_matches,
        })
    }
}

/// A conversation hint an adapter parsed but could not use.
///
/// Carried as data rather than logged where it was found, so the adapter needs no monitoring
/// handle and the application decides what is worth a counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnusableHint {
    /// An Outlook `Thread-Index` that would not canonicalize, and the encoded bytes it occupied.
    ThreadIndex(ThreadIndexParseError, usize),
}

const THREAD_INDEX_WARNING_INTERVAL_SECS: u64 = 60;
static LAST_THREAD_INDEX_WARNING_WINDOWS: [AtomicU64; 6] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

fn should_warn_about_thread_index(error: ThreadIndexParseError) -> bool {
    let window = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / THREAD_INDEX_WARNING_INTERVAL_SECS
        + 1;
    LAST_THREAD_INDEX_WARNING_WINDOWS[error.warning_slot()].swap(window, Ordering::Relaxed)
        != window
}
