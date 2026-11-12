//! Authorized simulation commands and their live stream.
use super::*;
/// GET /companies/{company_id}/channels/{id}/simulate - Simulation page (Protected).
#[instrument(skip_all)]
#[expect(
    clippy::too_many_arguments,
    reason = "Axum handlers receive request state and extractors as parameters"
)]
pub(super) async fn simulate_channel_page(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(outcome_reader): State<Arc<dyn MessageOutcomeReader>>,
    State(config): State<Arc<AppConfig>>,
    viewer: Viewer,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<SimulateQuery>,
) -> AppResult<Html<String>> {
    let (company, channel) = simulation_scope(
        &company_use_cases,
        &channel_use_cases,
        viewer.user_id,
        company_id,
        channel_id,
    )
    .await?;

    // A `?thread_id=` on the page URL pre-loads that conversation into the result pane.
    let requested_thread = query
        .thread_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty());
    let initial_result_html = match requested_thread {
        Some(thread_id) => Some(
            load_simulation_thread_fragment(
                &thread_use_cases,
                outcome_reader.as_ref(),
                SimulationTarget {
                    viewer: &viewer,
                    company: &company,
                    channel: &channel,
                },
                &config.app_domain_name,
                thread_id,
                false,
            )
            .await
            .map(|(thread, fragment)| {
                pages::simulation_live_view(company.id, channel.id, thread.id, &fragment)
            })?,
        ),
        None => None,
    };

    Ok(Html(pages::channel_simulation_page(
        &company,
        &config.app_domain_name,
        &channel,
        viewer.email.as_str(),
        requested_thread,
        initial_result_html.as_deref(),
    )))
}

/// The company and channel a simulation runs in. Simulation is the owner's tool -- `Run` mode
/// really sends -- so the company must be owned (404 for someone else's, 403 for an admin or
/// member), and the channel must be one of its channels.
async fn simulation_scope(
    company_use_cases: &CompanyUseCases,
    channel_use_cases: &ChannelUseCases,
    user_id: Uuid,
    company_id: Uuid,
    channel_id: Uuid,
) -> AppResult<(crate::entities::company::Company, Channel)> {
    let company = company_use_cases.owned_company(user_id, company_id).await?;
    let channel = channel_use_cases
        .get_company_channel(user_id, company_id, channel_id)
        .await?
        .ok_or_else(channel_not_found)?;
    Ok((company, channel))
}

/// Who is looking, and the authorized company and channel a simulated thread must belong to.
#[derive(Clone, Copy)]
struct SimulationTarget<'a> {
    viewer: &'a Viewer,
    company: &'a crate::entities::company::Company,
    channel: &'a Channel,
}

/// Read one scoped thread for a simulation. Missing, foreign and unreadable IDs are identical;
/// persistence errors propagate to the HTTP error response instead of becoming a successful fragment.
async fn load_simulation_thread_fragment(
    thread_use_cases: &ThreadUseCases,
    outcome_reader: &dyn MessageOutcomeReader,
    target: SimulationTarget<'_>,
    app_domain_name: &str,
    thread_id_input: &str,
    include_oob: bool,
) -> AppResult<(Thread, String)> {
    let SimulationTarget {
        viewer,
        company,
        channel,
    } = target;
    let thread_id = Uuid::parse_str(thread_id_input.trim())
        .map_err(|_| AppError::BadRequest("A valid thread UUID is required.".into()))?;
    let thread = thread_use_cases
        .readable_thread(viewer, company.id, channel.id, thread_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Thread not found".into()))?;
    let messages = thread_use_cases
        .get_thread_history(
            viewer,
            crate::use_cases::thread::ThreadReadScope::new(channel, thread.id),
        )
        .await?;
    let message_ids: Vec<Uuid> = messages
        .iter()
        .filter(|message| !message.is_agent())
        .map(|message| message.canonical_id.as_uuid())
        .collect();
    let outcomes = outcome_reader
        .message_outcomes(
            crate::application::message_outcomes::MessageOutcomeScope {
                company: crate::domain::workflow::CompanyId::new(company.id),
                thread_id: thread.id,
                viewer: crate::application::workflow::WorkflowActor::authenticated(viewer.user_id)?,
            },
            &message_ids,
        )
        .await?;
    let reply_context = thread_use_cases
        .viewer_email_reply_context(
            viewer,
            crate::use_cases::thread::ThreadReadScope::new(channel, thread.id),
        )
        .await?;
    let fragment = pages::channel_simulation_loaded_thread_fragment(&pages::SimulationThreadView {
        company,
        channel,
        app_domain_name,
        thread: &thread,
        messages: &messages,
        reply_context: reply_context.as_ref(),
        outcomes: &outcomes,
        include_oob,
    });
    Ok((thread, fragment))
}

/// URL the simulation page should show once a thread is open, so a refresh reloads the same thread.
fn simulation_thread_url(company_id: Uuid, channel_id: Uuid, thread_id: Uuid) -> String {
    format!("/companies/{company_id}/channels/{channel_id}/simulate?thread_id={thread_id}")
}

/// POST /companies/{company_id}/channels/{id}/simulate - Submit simulation form (Protected).
#[instrument(skip_all)]
#[expect(
    clippy::too_many_arguments,
    reason = "Axum handlers receive request state and extractors as parameters"
)]
pub(super) async fn simulate_channel_handler(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(outcome_reader): State<Arc<dyn MessageOutcomeReader>>,
    State(config): State<Arc<AppConfig>>,
    viewer: Viewer,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<SimulationForm>,
) -> AppResult<Response> {
    let (company, channel) = simulation_scope(
        &company_use_cases,
        &channel_use_cases,
        viewer.user_id,
        company_id,
        channel_id,
    )
    .await?;
    let command = SimulationCommand {
        company: &company,
        channel: &channel,
        form: &form,
        viewer: &viewer,
        config: &config,
        channels: &channel_use_cases,
        threads: &thread_use_cases,
        outcomes: outcome_reader.as_ref(),
    };
    if !simulation_recipient_matches(&form.to, &company, &channel, &config.app_domain_name) {
        return Ok(Html(pages::error_alert(
            "Simulation recipient must match the selected company and desk.",
        ))
        .into_response());
    }
    let mode = match form
        .simulation_mode
        .as_deref()
        .unwrap_or("run_test")
        .to_lowercase()
        .as_str()
    {
        "run_test" => SimulationMode::RunTest,
        "run" => SimulationMode::Run,
        _ => SimulationMode::Verify,
    };
    Ok(match mode {
        SimulationMode::Verify => command.verify().await,
        SimulationMode::RunTest | SimulationMode::Run => command.run(mode).await,
    })
}

struct SimulationCommand<'a> {
    company: &'a crate::entities::company::Company,
    channel: &'a Channel,
    form: &'a SimulationForm,
    viewer: &'a Viewer,
    config: &'a AppConfig,
    channels: &'a ChannelUseCases,
    threads: &'a ThreadUseCases,
    outcomes: &'a dyn MessageOutcomeReader,
}
struct PreviewRecipient {
    company: crate::entities::value_objects::CompanySlug,
    channel: crate::entities::value_objects::ChannelSlug,
    sender: crate::entities::transport::QualifiedIdentity,
}
impl SimulationCommand<'_> {
    fn failure(&self, sender: &str, message: impl AsRef<str>) -> Response {
        Html(pages::channel_simulation_failure_fragment(
            &pages::ChannelSimulationFailure {
                company_id: self.company.id,
                channel_id: self.channel.id,
                company: Some(self.company),
                channel: Some(self.channel),
                to: &self.form.to,
                from: sender,
                subject: self.form.subject.as_deref().unwrap_or("(No subject)"),
                error: message.as_ref(),
            },
        ))
        .into_response()
    }
    fn preview_recipient(&self, sender: &str) -> Result<PreviewRecipient, String> {
        let selection = EmailChannelSelectorParser::new(&self.config.app_domain_name)
            .parse(&self.form.to)
            .ok_or_else(|| "Invalid simulation recipient".to_string())?;
        let selector = selection
            .selectors()
            .first()
            .ok_or_else(|| "Invalid simulation recipient".to_string())?;
        let company = selector
            .company()
            .cloned()
            .ok_or_else(|| "Simulation recipient has no company".to_string())?;
        let sender = EmailIdentity::parse(EmailAddress::from(sender))
            .map_err(|error| error.to_string())?
            .qualify_default();
        Ok(PreviewRecipient {
            company,
            channel: selector.channel().clone(),
            sender,
        })
    }
    async fn verify(&self) -> Response {
        let sender = self.form.from.clone().unwrap_or_default();
        let recipient = match self.preview_recipient(&sender) {
            Ok(recipient) => recipient,
            Err(error) => return self.failure(&sender, error),
        };
        match self
            .channels
            .preview_inbound_route(recipient.company, recipient.channel, recipient.sender)
            .await
        {
            Ok(route) => {
                let result = pages::EmailSimulationResult {
                    resolved: route.resolved,
                    sender_authorized: route.sender_authorized,
                    company_slug: Some(route.company_slug),
                    channel_slug: Some(route.channel_slug),
                    company: route.company,
                    channel: route.channel,
                    to: self.form.to.clone(),
                    from: sender,
                    subject: self.form.subject.clone(),
                    text_body: self.form.text_body.clone(),
                };
                Html(pages::channel_simulation_result_fragment(
                    self.company.id,
                    self.channel.id,
                    &result,
                ))
                .into_response()
            }
            Err(error) => self.failure(&sender, format!("Simulation failed: {error}")),
        }
    }
    fn payload(&self, sender: &str) -> RawInboundPayload {
        RawInboundPayload {
            to: self.form.to.clone(),
            from: sender.to_string(),
            subject: self.form.subject.clone(),
            text: self.form.text_body.clone(),
            html: self.form.html_body.clone(),
            headers: reply_headers(self.form.in_reply_to.as_deref()),
            ..Default::default()
        }
    }
    async fn run(&self, mode: SimulationMode) -> Response {
        let sender = self.viewer.email.as_str();
        let ingest = match compose_and_ingest(
            self.threads,
            &self.config.app_domain_name,
            self.payload(sender),
            delivery_for(mode),
        )
        .await
        {
            Ok(ingest) => ingest,
            Err(error) => {
                return self.failure(sender, format!("Simulation execution failed: {error}"));
            }
        };
        let Some(thread) = ingest.thread.as_ref() else {
            return self.failure(
                sender,
                ingest.reason().unwrap_or("The desk rejected this message."),
            );
        };
        match load_simulation_thread_fragment(
            self.threads,
            self.outcomes,
            SimulationTarget {
                viewer: self.viewer,
                company: self.company,
                channel: self.channel,
            },
            &self.config.app_domain_name,
            &thread.id.to_string(),
            true,
        )
        .await
        {
            Ok((thread, fragment)) => (
                [(
                    "HX-Push-Url",
                    simulation_thread_url(self.company.id, self.channel.id, thread.id),
                )],
                Html(pages::simulation_live_view(
                    self.company.id,
                    self.channel.id,
                    thread.id,
                    &fragment,
                )),
            )
                .into_response(),
            Err(error) => error.into_response(),
        }
    }
}

/// GET /companies/{company_id}/channels/{id}/simulate/events - A simulated thread as the worker
/// works through it (Protected).
///
/// The simulation page used to run its agent inside the POST and print the result once. Now the run
/// is queued like every other message, so the page starts out showing only what was sent and fills
/// in from here. Every commit or status change on the thread re-renders the view whole: this is a
/// debugging page, and re-reading it in full is simpler and more honest than patching it in pieces.
#[instrument(skip_all)]
#[expect(
    clippy::too_many_arguments,
    reason = "Axum handlers receive request state and extractors as parameters"
)]
pub(super) async fn simulation_stream(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(outcome_reader): State<Arc<dyn MessageOutcomeReader>>,
    State(events): State<MailboxEvents>,
    State(config): State<Arc<AppConfig>>,
    viewer: Viewer,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
    Query(params): Query<OpenThreadParams>,
) -> AppResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    // Authorize once, here: the stream that follows filters on ids alone, so nothing past this
    // point re-checks ownership.
    let (company, channel) = simulation_scope(
        &company_use_cases,
        &channel_use_cases,
        viewer.user_id,
        company_id,
        channel_id,
    )
    .await?;
    let thread_id = params
        .thread_id
        .as_deref()
        .unwrap_or_default()
        .trim()
        .parse::<Uuid>()
        .map_err(|_| AppError::NotFound("Thread not found".into()))?;
    let thread = thread_use_cases
        .readable_thread(&viewer, company_id, channel_id, thread_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Thread not found".into()))?;

    let mut wake_ups = Box::pin(simulation_wake_ups(&events, "simulation", thread_id));
    let app_domain_name = config.app_domain_name.clone();

    let stream = async_stream::stream! {
        loop {
            // Re-read immediately on connect. Notifications are only wake-ups, so this also
            // recovers anything committed before the browser subscribed or while it was offline.
            // `include_oob: false` -- the out-of-band form reset belongs to the POST that started
            // this run, not to every update after it.
            let rendered = load_simulation_thread_fragment(
                &thread_use_cases,
                outcome_reader.as_ref(),
                SimulationTarget {
                    viewer: &viewer,
                    company: &company,
                    channel: &channel,
                },
                &app_domain_name,
                &thread.id.to_string(),
                false,
            )
            .await;
            match rendered {
                Ok((_, fragment)) => {
                    yield Ok(Event::default().event("simulation").data(fragment));
                }
                Err(_) => {
                    tracing::warn!("Simulation thread refresh failed; closing stream");
                    yield Ok(Event::default().event("simulation").data(pages::error_alert(
                        "The thread could not be refreshed. Reload the page to try again.",
                    )));
                    return;
                }
            }

            if wake_ups.next().await.is_none() {
                return;
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// Take in a message an authenticated route composed, and queue its agent run.
///
/// Returns as soon as the message is committed; the worker picks the task up on its next poll and
/// the reply reaches the open thread over the message stream -- the same route every piece of real
/// inbound mail takes.
///
/// The trust statement is the whole point of this being a separate entry. There is no email
/// transport at this boundary, so there is no DMARC result: the signed-in principal is the
/// authentication, and [`EmailIngressTrust::Application`] says so instead of handing the ingress
/// guard a `Pass` nobody verified. Callers must still derive the sender from the authenticated
/// account -- ingest enforces channel participant access, so a fabricated sender is refused, but it
/// is refused as a stranger rather than caught as a forgery.
pub(crate) async fn compose_and_ingest(
    thread_use_cases: &ThreadUseCases,
    app_domain_name: &str,
    payload: RawInboundPayload,
    delivery: ReplyDelivery,
) -> AppResult<InboundIngestResult> {
    let accepted = EmailIngressAdapter::new(app_domain_name)
        .accept(payload, EmailIngressTrust::Application)?;
    thread_use_cases
        .ingest(accepted.into_inbound(IngressOrigin::TrustedApplication, delivery))
        .await
}

/// A simulation asks for a real send only in `Run` mode; `RunTest` keeps the answer in-app.
fn delivery_for(mode: SimulationMode) -> ReplyDelivery {
    if mode == SimulationMode::Run {
        ReplyDelivery::Send
    } else {
        ReplyDelivery::InAppOnly
    }
}

/// Does the simulated `To:` address resolve to exactly the channel being simulated?
fn simulation_recipient_matches(
    to: &str,
    company: &crate::entities::company::Company,
    channel: &Channel,
    app_domain_name: &str,
) -> bool {
    EmailChannelSelectorParser::new(app_domain_name)
        .parse(to)
        .is_some_and(|selection| {
            let selectors = selection.selectors();
            !selectors.is_empty()
                && selectors.iter().all(|selector| {
                    selector
                        .company()
                        .is_some_and(|slug| slug.eq_ignore_ascii_case(&company.slug))
                        && selector.channel().eq_ignore_ascii_case(&channel.slug)
                })
        })
}

pub(super) struct OpenSimulatedThreadContext {
    pub(super) company_use_cases: Arc<CompanyUseCases>,
    pub(super) channel_use_cases: Arc<ChannelUseCases>,
    pub(super) thread_use_cases: Arc<ThreadUseCases>,
    pub(super) outcome_reader: Arc<dyn MessageOutcomeReader>,
    pub(super) config: Arc<AppConfig>,
    pub(super) viewer: Viewer,
    pub(super) company_id: Uuid,
    pub(super) channel_id: Uuid,
}

pub(super) async fn open_simulated_thread_logic(
    context: OpenSimulatedThreadContext,
    thread_id_input: &str,
) -> AppResult<Response> {
    let (company, channel) = simulation_scope(
        &context.company_use_cases,
        &context.channel_use_cases,
        context.viewer.user_id,
        context.company_id,
        context.channel_id,
    )
    .await?;

    Ok(
        match load_simulation_thread_fragment(
            &context.thread_use_cases,
            context.outcome_reader.as_ref(),
            SimulationTarget {
                viewer: &context.viewer,
                company: &company,
                channel: &channel,
            },
            &context.config.app_domain_name,
            thread_id_input,
            true,
        )
        .await
        {
            Ok((thread, fragment)) => (
                [(
                    "HX-Push-Url",
                    simulation_thread_url(context.company_id, context.channel_id, thread.id),
                )],
                Html(pages::simulation_live_view(
                    context.company_id,
                    context.channel_id,
                    thread.id,
                    &fragment,
                )),
            )
                .into_response(),
            Err(error) => return Err(error),
        },
    )
}

#[instrument(skip_all)]
#[expect(
    clippy::too_many_arguments,
    reason = "Axum handlers receive request state and extractors as parameters"
)]
pub(super) async fn open_simulated_thread_get(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(outcome_reader): State<Arc<dyn MessageOutcomeReader>>,
    State(config): State<Arc<AppConfig>>,
    viewer: Viewer,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
    Query(params): Query<OpenThreadParams>,
) -> AppResult<Response> {
    let tid_str = params.thread_id.as_deref().unwrap_or("");
    open_simulated_thread_logic(
        OpenSimulatedThreadContext {
            company_use_cases,
            channel_use_cases,
            thread_use_cases,
            outcome_reader,
            config,
            viewer,
            company_id,
            channel_id,
        },
        tid_str,
    )
    .await
}

#[instrument(skip_all)]
#[expect(
    clippy::too_many_arguments,
    reason = "Axum handlers receive request state and extractors as parameters"
)]
pub(super) async fn open_simulated_thread_post(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(outcome_reader): State<Arc<dyn MessageOutcomeReader>>,
    State(config): State<Arc<AppConfig>>,
    viewer: Viewer,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
    Form(params): Form<OpenThreadParams>,
) -> AppResult<Response> {
    let tid_str = params.thread_id.as_deref().unwrap_or("");
    open_simulated_thread_logic(
        OpenSimulatedThreadContext {
            company_use_cases,
            channel_use_cases,
            thread_use_cases,
            outcome_reader,
            config,
            viewer,
            company_id,
            channel_id,
        },
        tid_str,
    )
    .await
}
