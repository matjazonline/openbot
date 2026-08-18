//! `/ui` — the mailbox reader: channels on the left, that channel's threads next to them, and the
//! selected thread's messages on the right.
//!
//! Every handler here renders one column of [`crate::adapters::http::pages::mailbox`]; the columns
//! are swapped independently over htmx. Thread paging reuses the cursor helpers in
//! [`super::channel`] so both thread lists page identically.

use std::sync::Arc;

use axum::{
    Form, Router,
    extract::{Query, State},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;
use tracing::instrument;
use uuid::Uuid;

use crate::{
    adapters::http::{app_state::AppState, auth::AuthenticatedUser, pages},
    app_error::{AppError, AppResult},
    entities::{
        channel::Channel, company::Company, thread::Thread, user::User, value_objects::EmailAddress,
    },
    infra::config::AppConfig,
    services::email_parser::RawInboundPayload,
    use_cases::{
        channel::ChannelUseCases,
        company::CompanyUseCases,
        thread::{SimulationMode, ThreadUseCases},
        user::UserUseCases,
    },
};

use super::channel::{ThreadListQuery, ThreadListResponse, load_thread_page, reply_headers};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ui", get(mailbox_page))
        .route("/ui/threads", get(thread_column_fragment))
        .route("/ui/threads/list", get(thread_page_fragment))
        .route("/ui/messages", get(message_pane_fragment))
        .route("/ui/compose", get(compose_form).post(create_thread))
        .route("/ui/reply", get(reply_form).post(send_reply))
}

/// What the mailbox shell has selected, all optional so `/ui` alone is a valid entry point.
#[derive(Debug, Clone, Deserialize)]
pub struct MailboxQuery {
    pub company_id: Option<Uuid>,
    pub channel_id: Option<Uuid>,
    pub thread_id: Option<Uuid>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChannelQuery {
    pub company_id: Uuid,
    pub channel_id: Uuid,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ThreadPageQuery {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ThreadQuery {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Uuid,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ComposeForm {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub subject: Option<String>,
    pub text_body: Option<String>,
    /// Present only when the "deliver by email" toggle is on.
    pub deliver: Option<String>,
}

/// A further message in a thread that is already open; its subject comes from the thread.
#[derive(Debug, Clone, Deserialize)]
pub struct ReplyForm {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Uuid,
    pub text_body: Option<String>,
    /// Present only when the "deliver by email" toggle is on.
    pub deliver: Option<String>,
}

/// How the "deliver by email" toggle arrives on both send forms.
fn delivery_requested(deliver: Option<&str>) -> bool {
    matches!(deliver, Some("true") | Some("on"))
}

/// Delivering means running the channel for real; otherwise the agent's reply stays in-app.
fn delivery_mode(deliver: bool) -> SimulationMode {
    if deliver {
        SimulationMode::Run
    } else {
        SimulationMode::RunTest
    }
}

/// The company a mailbox request is scoped to, always picked from the caller's own companies so a
/// guessed `company_id` cannot reach another user's mail.
async fn load_scoped_company(
    company_use_cases: &CompanyUseCases,
    user_id: Uuid,
    requested: Option<Uuid>,
) -> AppResult<(Vec<Company>, Option<Company>)> {
    let companies = company_use_cases.list_user_companies(user_id).await?;
    let selected = match requested {
        Some(id) => companies.iter().find(|c| c.id == id).cloned(),
        None => companies.first().cloned(),
    };
    Ok((companies, selected))
}

/// Load a company and one of its channels, failing closed if either is not the caller's.
async fn load_company_channel(
    company_use_cases: &CompanyUseCases,
    channel_use_cases: &ChannelUseCases,
    user_id: Uuid,
    company_id: Uuid,
    channel_id: Uuid,
) -> AppResult<(Company, Channel)> {
    let channel = channel_use_cases
        .get_company_channel(user_id, company_id, channel_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Channel not found".into()))?;
    let company = company_use_cases
        .get_company(company_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Company not found".into()))?;
    Ok((company, channel))
}

/// A thread, but only when it really belongs to the channel the request claims.
async fn load_channel_thread(
    thread_use_cases: &ThreadUseCases,
    channel_id: Uuid,
    thread_id: Uuid,
) -> AppResult<Option<Thread>> {
    let thread = thread_use_cases.get_thread(thread_id).await?;
    Ok(thread.filter(|thread| thread.channel_id == channel_id))
}

fn empty_thread_page() -> ThreadListResponse {
    ThreadListResponse {
        threads: Vec::new(),
        next_cursor: None,
        has_more: false,
    }
}

async fn render_message_pane(
    thread_use_cases: &ThreadUseCases,
    company_id: Uuid,
    channel: &Channel,
    thread: &Thread,
) -> AppResult<String> {
    let messages = thread_use_cases.get_thread_history(thread.id).await?;
    Ok(pages::message_pane(&pages::MessagePane {
        company_id,
        channel,
        thread,
        messages: &messages,
    }))
}

/// GET /ui - The full mailbox shell for the selected company / channel / thread (Protected).
#[instrument(skip(
    company_use_cases,
    channel_use_cases,
    thread_use_cases,
    user_use_cases,
    config,
    user
))]
async fn mailbox_page(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Query(query): Query<MailboxQuery>,
) -> AppResult<Html<String>> {
    let account = load_account(&user_use_cases, user.id).await?;
    let account_email = EmailAddress::from(account.email.as_str());
    let mailbox_user = pages::MailboxUser {
        username: &account.username,
        email: &account_email,
    };

    let (companies, company) =
        load_scoped_company(&company_use_cases, user.id, query.company_id).await?;
    let Some(company) = company else {
        return Ok(Html(pages::mailbox_no_company_page(&mailbox_user)));
    };

    let channels = channel_use_cases
        .list_company_channels(user.id, company.id)
        .await?;
    let selected_channel = query
        .channel_id
        .and_then(|id| channels.iter().find(|channel| channel.id == id));

    let thread_page = match selected_channel {
        Some(channel) => {
            load_thread_page(&thread_use_cases, channel.id, &ThreadListQuery::default()).await?
        }
        None => empty_thread_page(),
    };

    let selected_thread = match (selected_channel, query.thread_id) {
        (Some(channel), Some(thread_id)) => {
            load_channel_thread(&thread_use_cases, channel.id, thread_id).await?
        }
        _ => None,
    };

    let detail_html = match (selected_channel, &selected_thread) {
        (Some(channel), Some(thread)) => {
            render_message_pane(&thread_use_cases, company.id, channel, thread).await?
        }
        (Some(_), None) => pages::empty_detail_pane(
            "Select a thread, or use Compose to start a new one.",
            pages::FragmentSwap::Inline,
        ),
        (None, _) => pages::empty_detail_pane(
            "Select a channel to get started.",
            pages::FragmentSwap::Inline,
        ),
    };

    Ok(Html(pages::mailbox_page(&pages::MailboxPage {
        user: &mailbox_user,
        company: &company,
        companies: &companies,
        app_domain_name: &config.app_domain_name,
        channels: &channels,
        selected_channel,
        threads: &thread_page.threads,
        next_cursor: thread_page.next_cursor.as_deref(),
        selected_thread_id: selected_thread.as_ref().map(|thread| thread.id),
        detail_html: &detail_html,
    })))
}

/// GET /ui/threads - The thread column for a channel, clearing the detail pane (Protected).
#[instrument(skip(channel_use_cases, thread_use_cases, user))]
async fn thread_column_fragment(
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    user: AuthenticatedUser,
    Query(query): Query<ChannelQuery>,
) -> AppResult<Html<String>> {
    let channel = channel_use_cases
        .get_company_channel(user.id, query.company_id, query.channel_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Channel not found".into()))?;

    let page = load_thread_page(&thread_use_cases, channel.id, &ThreadListQuery::default()).await?;

    let column = pages::thread_column(&pages::ThreadColumn {
        company_id: query.company_id,
        channel: &channel,
        threads: &page.threads,
        next_cursor: page.next_cursor.as_deref(),
        selected_thread_id: None,
    });
    let detail = pages::empty_detail_pane(
        "Select a thread, or use Compose to start a new one.",
        pages::FragmentSwap::OutOfBand,
    );

    Ok(Html(format!("{column}{detail}")))
}

/// GET /ui/threads/list - One older page of threads, appended to the open column (Protected).
#[instrument(skip(channel_use_cases, thread_use_cases, user))]
async fn thread_page_fragment(
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    user: AuthenticatedUser,
    Query(query): Query<ThreadPageQuery>,
) -> AppResult<Html<String>> {
    let channel = channel_use_cases
        .get_company_channel(user.id, query.company_id, query.channel_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Channel not found".into()))?;

    let page = load_thread_page(
        &thread_use_cases,
        channel.id,
        &ThreadListQuery {
            limit: query.limit,
            cursor: query.cursor,
        },
    )
    .await?;

    Ok(Html(pages::thread_list_fragment(
        &pages::ThreadColumn {
            company_id: query.company_id,
            channel: &channel,
            threads: &page.threads,
            next_cursor: page.next_cursor.as_deref(),
            selected_thread_id: None,
        },
        pages::FragmentSwap::OutOfBand,
    )))
}

/// GET /ui/messages - The messages of one thread (Protected).
#[instrument(skip(channel_use_cases, thread_use_cases, user))]
async fn message_pane_fragment(
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    user: AuthenticatedUser,
    Query(query): Query<ThreadQuery>,
) -> AppResult<Html<String>> {
    let channel = channel_use_cases
        .get_company_channel(user.id, query.company_id, query.channel_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Channel not found".into()))?;
    let thread = load_channel_thread(&thread_use_cases, channel.id, query.thread_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Thread not found".into()))?;

    Ok(Html(
        render_message_pane(&thread_use_cases, query.company_id, &channel, &thread).await?,
    ))
}

/// GET /ui/compose - The new-thread form for the selected channel (Protected).
#[instrument(skip(company_use_cases, channel_use_cases, user_use_cases, config, user))]
async fn compose_form(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Query(query): Query<ChannelQuery>,
) -> AppResult<Html<String>> {
    let (company, channel) = load_company_channel(
        &company_use_cases,
        &channel_use_cases,
        user.id,
        query.company_id,
        query.channel_id,
    )
    .await?;
    let sender_email = sender_email(&user_use_cases, user.id).await?;

    Ok(Html(pages::compose_pane(&pages::ComposePane {
        company_id: company.id,
        channel: &channel,
        channel_address: &channel.inbound_address(&company.slug, &config.app_domain_name),
        sender_email: &sender_email,
        subject: "",
        text_body: "",
        deliver: false,
        error: None,
    })))
}

/// POST /ui/compose - Start a new thread by feeding the composed message into the channel.
///
/// The message takes exactly the inbound path a real email would, so channel rules (participants,
/// spam, agents) apply unchanged; the toggle only decides whether the agent's reply is actually
/// delivered by email.
#[instrument(skip(
    company_use_cases,
    channel_use_cases,
    thread_use_cases,
    user_use_cases,
    config,
    user,
    form
))]
async fn create_thread(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Form(form): Form<ComposeForm>,
) -> AppResult<Response> {
    let (company, channel) = load_company_channel(
        &company_use_cases,
        &channel_use_cases,
        user.id,
        form.company_id,
        form.channel_id,
    )
    .await?;
    let sender_email = sender_email(&user_use_cases, user.id).await?;
    let address = channel.inbound_address(&company.slug, &config.app_domain_name);

    let subject = form.subject.unwrap_or_default();
    let text_body = form.text_body.unwrap_or_default();
    let deliver = delivery_requested(form.deliver.as_deref());

    let compose_error = |message: String| {
        Html(pages::compose_pane(&pages::ComposePane {
            company_id: company.id,
            channel: &channel,
            channel_address: address.as_str(),
            sender_email: &sender_email,
            subject: &subject,
            text_body: &text_body,
            deliver,
            error: Some(&message),
        }))
        .into_response()
    };

    if subject.trim().is_empty() || text_body.trim().is_empty() {
        return Ok(compose_error(
            "Subject and message are both required.".to_string(),
        ));
    }

    let payload = RawInboundPayload {
        to: address.to_string(),
        from: sender_email.clone(),
        subject: Some(subject.clone()),
        text: Some(text_body.clone()),
        ..Default::default()
    };
    let result = match thread_use_cases
        .execute_simulation(payload, delivery_mode(deliver))
        .await
    {
        Ok(result) => result,
        Err(err) => return Ok(compose_error(format!("Failed to send message: {err}"))),
    };

    let Some(thread) = result.ingest_result.thread else {
        let reason = result
            .ingest_result
            .reason
            .unwrap_or_else(|| "The channel rejected this message.".to_string());
        return Ok(compose_error(reason));
    };

    sent_message_response(&thread_use_cases, company.id, &channel, &thread).await
}

/// GET /ui/reply - The new-message form for the open thread (Protected).
#[instrument(skip(
    company_use_cases,
    channel_use_cases,
    thread_use_cases,
    user_use_cases,
    config,
    user
))]
async fn reply_form(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Query(query): Query<ThreadQuery>,
) -> AppResult<Html<String>> {
    let (company, channel) = load_company_channel(
        &company_use_cases,
        &channel_use_cases,
        user.id,
        query.company_id,
        query.channel_id,
    )
    .await?;
    let thread = load_channel_thread(&thread_use_cases, channel.id, query.thread_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Thread not found".into()))?;
    let sender_email = sender_email(&user_use_cases, user.id).await?;

    Ok(Html(pages::reply_pane(&pages::ReplyPane {
        company_id: company.id,
        channel: &channel,
        thread: &thread,
        channel_address: &channel.inbound_address(&company.slug, &config.app_domain_name),
        sender_email: &sender_email,
        text_body: "",
        deliver: false,
        error: None,
    })))
}

/// POST /ui/reply - Send a further message into an open thread.
///
/// Like Compose, the message takes the inbound path a real email would; the threading headers are
/// what keep it in this conversation instead of starting a new one.
#[instrument(skip(
    company_use_cases,
    channel_use_cases,
    thread_use_cases,
    user_use_cases,
    config,
    user,
    form
))]
async fn send_reply(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Form(form): Form<ReplyForm>,
) -> AppResult<Response> {
    let (company, channel) = load_company_channel(
        &company_use_cases,
        &channel_use_cases,
        user.id,
        form.company_id,
        form.channel_id,
    )
    .await?;
    let thread = load_channel_thread(&thread_use_cases, channel.id, form.thread_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Thread not found".into()))?;
    let sender_email = sender_email(&user_use_cases, user.id).await?;
    let address = channel.inbound_address(&company.slug, &config.app_domain_name);

    let text_body = form.text_body.unwrap_or_default();
    let deliver = delivery_requested(form.deliver.as_deref());

    let reply_error = |message: String| {
        Html(pages::reply_pane(&pages::ReplyPane {
            company_id: company.id,
            channel: &channel,
            thread: &thread,
            channel_address: address.as_str(),
            sender_email: &sender_email,
            text_body: &text_body,
            deliver,
            error: Some(&message),
        }))
        .into_response()
    };

    if text_body.trim().is_empty() {
        return Ok(reply_error("A message is required.".to_string()));
    }

    // Threading is by header, so the reply hangs off the newest message the thread already has.
    let history = thread_use_cases.get_thread_history(thread.id).await?;
    let in_reply_to = history.last().map(|message| message.message_id.as_str());

    let payload = RawInboundPayload {
        to: address.to_string(),
        from: sender_email.clone(),
        subject: Some(thread.reply_subject()),
        text: Some(text_body.clone()),
        headers: reply_headers(in_reply_to),
        ..Default::default()
    };

    let result = match thread_use_cases
        .execute_simulation(payload, delivery_mode(deliver))
        .await
    {
        Ok(result) => result,
        Err(err) => return Ok(reply_error(format!("Failed to send message: {err}"))),
    };

    let Some(sent_thread) = result.ingest_result.thread else {
        let reason = result
            .ingest_result
            .reason
            .unwrap_or_else(|| "The channel rejected this message.".to_string());
        return Ok(reply_error(reason));
    };

    sent_message_response(&thread_use_cases, company.id, &channel, &sent_thread).await
}

/// What both send forms return: the thread's messages, with its column refreshed beside them.
async fn sent_message_response(
    thread_use_cases: &ThreadUseCases,
    company_id: Uuid,
    channel: &Channel,
    thread: &Thread,
) -> AppResult<Response> {
    let pane = render_message_pane(thread_use_cases, company_id, channel, thread).await?;
    let page = load_thread_page(thread_use_cases, channel.id, &ThreadListQuery::default()).await?;
    let refreshed_list = pages::thread_list_oob(&pages::ThreadColumn {
        company_id,
        channel,
        threads: &page.threads,
        next_cursor: page.next_cursor.as_deref(),
        selected_thread_id: Some(thread.id),
    });

    Ok((
        [(
            "HX-Push-Url",
            format!(
                "/ui?company_id={}&channel_id={}&thread_id={}",
                company_id, channel.id, thread.id
            ),
        )],
        Html(format!("{pane}{refreshed_list}")),
    )
        .into_response())
}

/// The signed-in account itself, needed both for the top bar and as the sender of composed mail.
async fn load_account(user_use_cases: &UserUseCases, user_id: Uuid) -> AppResult<User> {
    user_use_cases
        .get_user_by_id(user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("User not found".into()))
}

/// The signed-in user's own address — composed mail is always sent as them.
async fn sender_email(user_use_cases: &UserUseCases, user_id: Uuid) -> AppResult<String> {
    Ok(load_account(user_use_cases, user_id).await?.email)
}
