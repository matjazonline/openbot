use super::*;
use crate::application::transport::{
    DeliveryComposer, DeliveryContext, DeliveryPurpose, DeliveryRequest, EmailDeliveryContext,
    EmailRelayTrace, EmailThreading,
};
use crate::application::use_cases::thread::{
    MessageAuthorWrite, MessageCorrelation, MessageParticipantWrite, qualified_email_identity,
};
use crate::application::workflow::lease::{
    LeasePolicy, SuspendedWorkflow, WorkflowFailure, WorkflowHandlerOutcome,
};
use crate::application::workflow::worker::WorkflowHandler;
use crate::domain::workflow::{FailureClass, FailureCode, RetrySafety, StepFailure};
use crate::entities::channel::Channel;
use crate::entities::email_message::EmailMessageMetadata;
use crate::entities::message::{
    MessageDirection, MessageParticipantKind, MessageRole, ThreadEntryKind,
};

/// `message.send`/`message.reply` steps and direct message tools share one invocation path.
pub struct MessageActions<P, D> {
    persistence: P,
    dispatch: D,
    composer: DeliveryComposer,
    app_domain_name: String,
    lease: LeasePolicy,
}
impl<P, D> MessageActions<P, D> {
    pub fn new(
        persistence: P,
        dispatch: D,
        composer: DeliveryComposer,
        app_domain_name: String,
        lease: LeasePolicy,
    ) -> Self {
        Self {
            persistence,
            dispatch,
            composer,
            app_domain_name,
            lease,
        }
    }
}

enum StepError {
    Preparation(AppError),
    Dispatch(AppError),
}

impl<P, D> MessageActions<P, D>
where
    P: MessageActionPreparation + ActionIntents + Clone,
    D: MessageDispatch,
{
    /// Direct tool entry. Re-entry with the same model call observes the same invocation.
    pub async fn call_tool(
        &self,
        fence: WorkflowFence,
        call: MessageToolCall,
    ) -> AppResult<MessageObservation> {
        let request = self
            .persistence
            .message_tool_request(tool_scope(fence), call.kind, call.arguments)
            .await?;
        let arguments = MessageArguments::parse(call.kind, &request.arguments)?;
        let intent = ActionService::new(self.persistence.clone())
            .prepare_tool(request.clone(), call.call)
            .await?;
        let dispatch = self.dispatch_request(&intent, fence);
        let publication =
            Box::pin(self.publish(&request, &arguments, &dispatch, MessageOrigin::ModelTool))
                .await
                .map_err(|error| match error {
                    StepError::Preparation(error) | StepError::Dispatch(error) => error,
                })?;
        Ok(match publication {
            MessagePublication::Rejected(error) => return Err(error),
            MessagePublication::Committed(receipt) => MessageObservation::Committed(receipt),
            MessagePublication::Suspended(wait) => MessageObservation::DeliveryParked(wait),
            MessagePublication::Pending | MessagePublication::PossibleDispatchExists => {
                MessageObservation::Pending
            }
            MessagePublication::Uncertain => MessageObservation::Uncertain,
            MessagePublication::ApprovalRequired => MessageObservation::ApprovalRequired,
            MessagePublication::ApprovalParked(wait) => MessageObservation::ApprovalParked(wait),
            MessagePublication::ActionLimit => MessageObservation::ActionLimit,
        })
    }

    fn dispatch_request(
        &self,
        intent: &ActionIntent,
        fence: WorkflowFence,
    ) -> ActionDispatchRequest {
        ActionDispatchRequest {
            subject: intent.approval_subject(),
            fence,
            lease: self.lease,
        }
    }

    async fn step(
        &self,
        kind: &WorkflowStepKind,
        claim: &ClaimedWorkflow,
    ) -> Result<MessagePublication, StepError> {
        let message_kind =
            MessageKind::parse(&kind.0).ok_or_else(|| StepError::Preparation(invalid_message()))?;
        let request = self
            .persistence
            .message_step_request(claim, kind)
            .await
            .map_err(StepError::Preparation)?;
        let arguments = MessageArguments::parse(message_kind, &request.arguments)
            .map_err(StepError::Preparation)?;
        let intent = ActionService::new(self.persistence.clone())
            .prepare_step(request.clone())
            .await
            .map_err(StepError::Preparation)?;
        let dispatch = self.dispatch_request(&intent, claim.fence);
        self.publish(&request, &arguments, &dispatch, MessageOrigin::Step)
            .await
    }

    /// Replay first, so a committed invocation never depends on routes that changed later.
    async fn publish(
        &self,
        request: &ActionRequest,
        arguments: &MessageArguments,
        dispatch: &ActionDispatchRequest,
        origin: MessageOrigin,
    ) -> Result<MessagePublication, StepError> {
        if let Some(existing) = self
            .dispatch
            .observe(dispatch, origin)
            .await
            .map_err(StepError::Dispatch)?
        {
            return Ok(existing);
        }
        let routes = self
            .persistence
            .message_routes(request, arguments)
            .await
            .map_err(StepError::Preparation)?;
        let plan = self
            .plan(request, dispatch, arguments, &routes)
            .await
            .map_err(StepError::Preparation)?;
        self.dispatch
            .publish(dispatch, origin, &plan)
            .await
            .map_err(StepError::Dispatch)
    }

    async fn plan(
        &self,
        request: &ActionRequest,
        dispatch: &ActionDispatchRequest,
        arguments: &MessageArguments,
        routes: &MessageRoutes,
    ) -> AppResult<MessagePlan> {
        let content = arguments.content();
        let source_key = format!("invocation:{}", dispatch.subject.invocation.as_uuid());
        let mut destinations = Vec::with_capacity(routes.targets.len());
        let ids: Vec<(DestinationKind, Uuid)> = match arguments {
            MessageArguments::Send { destinations, .. } => destinations
                .iter()
                .map(|destination| (destination.kind, destination.id))
                .collect(),
            MessageArguments::Reply { source, .. } => {
                vec![(DestinationKind::Reply, source.as_uuid())]
            }
        };
        if ids.len() != routes.targets.len() {
            return Err(invalid_message());
        }
        for ((kind, destination_id), target) in ids.into_iter().zip(&routes.targets) {
            let planned = match target {
                RouteTarget::Channel { channel } => {
                    plan_channel(kind, destination_id, *channel, content, routes)
                }
                RouteTarget::ReplyInternal {
                    source,
                    thread,
                    channel,
                } => plan_internal_reply(*source, *thread, *channel, content, routes),
                RouteTarget::User { .. } | RouteTarget::ReplyEmail { .. } => {
                    self.plan_email(
                        request,
                        (kind, destination_id),
                        target,
                        content,
                        routes,
                        &source_key,
                    )
                    .await?
                }
            };
            destinations.push(planned);
        }
        Ok(MessagePlan { destinations })
    }

    async fn plan_email(
        &self,
        request: &ActionRequest,
        (kind, destination_id): (DestinationKind, Uuid),
        target: &RouteTarget,
        content: &CanonicalContent,
        routes: &MessageRoutes,
        source_key: &str,
    ) -> AppResult<PlannedDestination> {
        let email = EmailPlan::of(target, content)?;
        let from = Channel::address_for(
            &email.sender.slug,
            &routes.company_slug,
            &self.app_domain_name,
        );
        let message_id = CanonicalMessageId::random();
        let composed = self
            .composer
            .compose(DeliveryRequest {
                company_id: request.scope.company.as_uuid(),
                channel_id: email.sender.id,
                message_id,
                task_id: None,
                correlation_id: routes.correlation_id,
                purpose: email.purpose,
                source_key: source_key.to_string(),
                content,
                context: DeliveryContext::Email(EmailDeliveryContext {
                    from: from.clone(),
                    from_name: Some(email.sender.name.clone()),
                    recipient_to: email.recipient.clone(),
                    recipients_cc: Vec::new(),
                    threading: email.threading.clone(),
                    relay: Some(email.relay.clone()),
                }),
            })
            .await
            .map_err(|error| {
                AppError::BadRequest(format!("Message delivery cannot be composed: {error}"))
            })?;
        let provider_key = composed.provider_key.ok_or_else(|| {
            AppError::BadRequest("Email delivery has no predicted Message-ID".into())
        })?;
        let (in_reply_to, references) = match &email.threading {
            EmailThreading::Received {
                in_reply_to,
                references,
            } => (Some(in_reply_to.clone()), references.clone()),
            EmailThreading::Standalone | EmailThreading::Anchored(_) => (None, Vec::new()),
        };
        let message = MessageWrite {
            id: message_id,
            ..MessageWrite::internal(
                email.thread_id,
                MessageAuthorWrite::Platform,
                content.subject().to_string(),
                content.body_text().to_string(),
                MessageDirection::Outbound,
                MessageRole::System,
                routes.correlation_id,
            )
        }
        .external_conversation()
        .with_participants(vec![
            MessageParticipantWrite::new(
                MessageParticipantKind::Sender,
                qualified_email_identity(from.as_str())?,
            ),
            MessageParticipantWrite::new(
                MessageParticipantKind::To,
                qualified_email_identity(email.recipient.as_str())?,
            ),
        ])
        .with_correlation(MessageCorrelation::Email(
            EmailMessageMetadata::new(MessageId::from(provider_key.as_str().to_string()))
                .in_reply_to(in_reply_to)
                .references(references)
                .raw_bodies(Some(content.body_text().to_string()), None),
        ));
        Ok(PlannedDestination {
            kind,
            destination_id,
            new_thread: email.new_thread,
            thread_id: email.thread_id,
            message,
            delivery: Some(composed.delivery),
            principal_id: email.principal,
            recipient: Some(email.recipient),
            check: email.check,
        })
    }
}

/// The email-specific facts of one destination, decided without I/O.
struct EmailPlan {
    sender: ChannelRoute,
    recipient: EmailAddress,
    threading: EmailThreading,
    relay: EmailRelayTrace,
    purpose: DeliveryPurpose,
    thread_id: Uuid,
    new_thread: Option<NewThread>,
    principal: Option<Uuid>,
    check: RouteCheck,
}
impl EmailPlan {
    fn of(target: &RouteTarget, content: &CanonicalContent) -> AppResult<Self> {
        match target {
            RouteTarget::User {
                user,
                principal,
                email,
                sender,
            } => {
                // A user email always opens its own thread in the run channel: it never joins
                // (and so never answers or re-threads) a customer conversation.
                let thread_id = Uuid::new_v4();
                Ok(Self {
                    sender: sender.clone(),
                    recipient: email.clone(),
                    threading: EmailThreading::Standalone,
                    // The scheduled-delivery precedent: the sending channel starts the chain.
                    relay: EmailRelayTrace {
                        source_channel_id: sender.id,
                        hop_count: 0,
                        trace_channels: vec![sender.id],
                    },
                    purpose: DeliveryPurpose::WorkflowSend,
                    thread_id,
                    new_thread: Some(NewThread {
                        id: thread_id,
                        channel_id: sender.id,
                        subject: content.subject().to_string(),
                    }),
                    principal: Some(*principal),
                    check: RouteCheck::Member {
                        user: *user,
                        principal: *principal,
                        email: email.clone(),
                        sender: sender.id,
                    },
                })
            }
            RouteTarget::ReplyEmail {
                source,
                thread,
                sender,
                recipient,
                in_reply_to,
                references,
                relay,
            } => Ok(Self {
                sender: sender.clone(),
                recipient: recipient.clone(),
                threading: EmailThreading::Received {
                    in_reply_to: in_reply_to.clone(),
                    references: references.clone(),
                },
                // The agent-answer precedent (`dispatch.rs`): the reply continues the trace the
                // source arrived with; with none recorded it starts one, as direct mail does.
                relay: EmailRelayTrace {
                    source_channel_id: sender.id,
                    hop_count: relay.as_ref().map_or(0, |relay| relay.hop_count),
                    trace_channels: relay
                        .as_ref()
                        .map(|relay| relay.trace_channels.clone())
                        .unwrap_or_default(),
                },
                purpose: DeliveryPurpose::WorkflowReply,
                thread_id: *thread,
                new_thread: None,
                principal: None,
                check: RouteCheck::Source {
                    source: *source,
                    thread: *thread,
                    channel: sender.id,
                },
            }),
            RouteTarget::Channel { .. } | RouteTarget::ReplyInternal { .. } => {
                Err(invalid_message())
            }
        }
    }
}

fn plan_channel(
    kind: DestinationKind,
    destination_id: Uuid,
    channel: Uuid,
    content: &CanonicalContent,
    routes: &MessageRoutes,
) -> PlannedDestination {
    let thread_id = Uuid::new_v4();
    let message = MessageWrite::internal(
        thread_id,
        MessageAuthorWrite::Platform,
        content.subject().to_string(),
        content.body_text().to_string(),
        MessageDirection::Outbound,
        MessageRole::System,
        routes.correlation_id,
    )
    // It opens its own thread, so a conversation entry answers no earlier turn.
    .with_entry_kind(ThreadEntryKind::Conversation);
    PlannedDestination {
        kind,
        destination_id,
        new_thread: Some(NewThread {
            id: thread_id,
            channel_id: channel,
            subject: content.subject().to_string(),
        }),
        thread_id,
        message,
        delivery: None,
        principal_id: None,
        recipient: None,
        check: RouteCheck::Channel { channel },
    }
}

fn plan_internal_reply(
    source: CanonicalMessageId,
    thread: Uuid,
    channel: Uuid,
    content: &CanonicalContent,
    routes: &MessageRoutes,
) -> PlannedDestination {
    // A note: it neither answers a customer turn nor carries email headers.
    let message = MessageWrite::internal(
        thread,
        MessageAuthorWrite::Platform,
        content.subject().to_string(),
        content.body_text().to_string(),
        MessageDirection::Outbound,
        MessageRole::System,
        routes.correlation_id,
    )
    .with_entry_kind(ThreadEntryKind::Note);
    PlannedDestination {
        kind: DestinationKind::Reply,
        destination_id: source.as_uuid(),
        new_thread: None,
        thread_id: thread,
        message,
        delivery: None,
        principal_id: None,
        recipient: None,
        check: RouteCheck::Source {
            source,
            thread,
            channel,
        },
    }
}

fn failure(class: FailureClass, code: &'static str) -> WorkflowFailure {
    WorkflowFailure {
        failure: StepFailure::new(
            class,
            FailureCode::parse(code).expect("static message failure code"),
            None,
        )
        .expect("bounded message failure"),
        safety: RetrySafety::SafeToRetry,
    }
}

/// A failed receipt is a classified terminal step failure; nothing is resent.
pub fn message_delivery_failed() -> WorkflowFailure {
    failure(FailureClass::Terminal, "message.delivery_failed")
}

#[async_trait]
impl<P, D> WorkflowHandler for MessageActions<P, D>
where
    P: MessageActionPreparation + ActionIntents + Clone,
    D: MessageDispatch,
{
    fn supports(&self, kind: &WorkflowStepKind) -> bool {
        MessageKind::parse(&kind.0).is_some()
    }
    async fn execute(
        &self,
        kind: &WorkflowStepKind,
        claim: &ClaimedWorkflow,
    ) -> WorkflowHandlerOutcome {
        use ActionStepOutcome as Outcome;
        let family = ActionFamily::Message;
        let outcome = match self.step(kind, claim).await {
            Ok(MessagePublication::Committed(receipt)) => {
                return if receipt.result.get("status").and_then(Value::as_str) == Some("accepted") {
                    WorkflowHandlerOutcome::Completed(Ok(receipt.result))
                } else {
                    WorkflowHandlerOutcome::Completed(Err(message_delivery_failed()))
                };
            }
            Ok(MessagePublication::Suspended(wait) | MessagePublication::ApprovalParked(wait)) => {
                return WorkflowHandlerOutcome::Suspended(SuspendedWorkflow { wait });
            }
            // Refused before any marker: classified exactly as a preparation refusal.
            Ok(MessagePublication::Rejected(error)) => {
                return WorkflowHandlerOutcome::Completed(Err(action_step_failure(
                    family,
                    Outcome::Preparation(&error),
                )));
            }
            Ok(MessagePublication::ApprovalRequired) => Outcome::ApprovalRequired,
            Ok(MessagePublication::ActionLimit) => Outcome::ActionLimit,
            Ok(
                MessagePublication::Pending
                | MessagePublication::Uncertain
                | MessagePublication::PossibleDispatchExists,
            ) => Outcome::PossibleDispatchExists,
            Err(StepError::Preparation(error)) => {
                return WorkflowHandlerOutcome::Completed(Err(action_step_failure(
                    family,
                    Outcome::Preparation(&error),
                )));
            }
            Err(StepError::Dispatch(error)) => {
                return WorkflowHandlerOutcome::Completed(Err(action_step_failure(
                    family,
                    Outcome::Dispatch(&error),
                )));
            }
        };
        WorkflowHandlerOutcome::Completed(Err(action_step_failure(family, outcome)))
    }
}
