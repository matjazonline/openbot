//! `/ui` — the mailbox reader: channels on the left, that channel's threads next to them, and the
//! selected thread's messages on the right.
//!
//! Every handler here renders one column of [`crate::adapters::http::pages::mailbox`]; the columns
//! are swapped independently over htmx. Thread paging reuses the cursor helpers in
//! [`super::channel`] so both thread lists page identically.

use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    str::FromStr,
    sync::Arc,
};

use axum::{
    Form, Router,
    extract::{Query, State},
    http::HeaderMap,
    response::{
        Html, IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::get,
};
use serde::Deserialize;
use tokio_stream::{Stream, StreamExt};
use tracing::{instrument, warn};
use uuid::Uuid;

use crate::{
    adapters::http::{app_state::AppState, auth::AuthenticatedUser, pages},
    app_error::{AppError, AppResult},
    entities::{
        agent::Agent,
        channel::Channel,
        company::{Company, CompanyAccess},
        company_member::CompanyMembership,
        task::ThreadActivity,
        thread::Thread,
        user::{User, Viewer},
        value_objects::EmailAddress,
    },
    infra::{
        config::AppConfig,
        events::{MailboxEvent, MailboxEvents},
    },
    services::email_parser::RawInboundPayload,
    use_cases::{
        agent::AgentUseCases,
        channel::ChannelUseCases,
        company::CompanyUseCases,
        thread::{ReplyDelivery, ThreadUseCases},
        user::UserUseCases,
    },
};

use super::{
    channel::{ThreadListQuery, ThreadListResponse, load_thread_page, reply_headers},
    live_updates::{Wake, channel_wake_ups, thread_wake_ups},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ui", get(mailbox_page))
        .route("/ui/threads", get(thread_column_fragment))
        .route("/ui/threads/list", get(thread_page_fragment))
        .route("/ui/messages", get(message_pane_fragment))
        .route("/ui/events", get(thread_message_stream))
        .route("/ui/threads/events", get(thread_column_stream))
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

/// A channel whose thread column to stream, plus the newest thread the column was rendered with.
#[derive(Debug, Clone, Deserialize)]
pub struct ChannelStreamQuery {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub after: Option<String>,
}

/// A thread to stream, plus the newest message the page was rendered with.
#[derive(Debug, Clone, Deserialize)]
pub struct ThreadStreamQuery {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Uuid,
    pub after: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ComposeForm {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub subject: Option<String>,
    pub text_body: Option<String>,
    /// Present only when the "deliver by email" toggle is on.
    pub deliver: Option<String>,
    /// Present only when the message should be stored without running the agent.
    pub quiet: Option<String>,
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
    /// Present only when the message should be stored without running the agent.
    pub quiet: Option<String>,
}

/// How the "deliver by email" toggle arrives on both send forms.
fn delivery_requested(deliver: Option<&str>) -> bool {
    matches!(deliver, Some("true") | Some("on"))
}

/// Quiet messages use the same explicit context-only marker accepted by inbound email.
fn message_body(text_body: &str, quiet: bool) -> String {
    if quiet {
        format!("[[quiet]] {text_body}")
    } else {
        text_body.to_string()
    }
}

/// Delivering means the agent's reply is really emailed; otherwise it stays in the mailbox.
fn delivery_mode(deliver: bool) -> ReplyDelivery {
    if deliver {
        ReplyDelivery::Send
    } else {
        ReplyDelivery::InAppOnly
    }
}

/// The company a mailbox request is scoped to, always picked from the caller's own companies so a
/// guessed `company_id` cannot reach another user's mail.
pub(super) async fn load_scoped_company(
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

/// The company an operational or automation-management request is scoped to.
///
/// Owners and company admins may configure these resources. Ordinary members are intentionally
/// absent from both the returned list and the selection, so a guessed company id grants nothing.
pub(super) async fn load_managed_company(
    company_use_cases: &CompanyUseCases,
    user_id: Uuid,
    requested: Option<Uuid>,
) -> AppResult<(Vec<Company>, Option<Company>)> {
    let companies = company_use_cases.list_managed_companies(user_id).await?;
    let selected = match requested {
        Some(id) => companies.iter().find(|company| company.id == id).cloned(),
        None => companies.first().cloned(),
    };
    Ok((companies, selected))
}

/// Membership implied by a successful [`load_managed_company`] selection.
pub(super) fn managed_company_membership(company: &Company, user_id: Uuid) -> CompanyMembership {
    if company.user_id == user_id {
        CompanyMembership::Owner
    } else {
        CompanyMembership::Admin
    }
}

/// The company a *read* is scoped to, from the companies the caller may see rather than only the
/// ones they own -- an invited member reads their team's mail, but still administers nothing.
pub(super) async fn load_readable_company(
    company_use_cases: &CompanyUseCases,
    user_id: Uuid,
    requested: Option<Uuid>,
) -> AppResult<(Vec<Company>, Option<CompanyAccess>)> {
    let accessible = company_use_cases.list_accessible_companies(user_id).await?;
    let selected = match requested {
        Some(id) => accessible.iter().find(|a| a.company.id == id).cloned(),
        None => accessible.first().cloned(),
    };

    let companies = accessible.into_iter().map(|a| a.company).collect();
    Ok((companies, selected))
}

/// A company and one of its channels, for anyone the channel lets in.
///
/// Fails closed through [`Channel::viewer_access`]: a channel with its own participant list is not
/// the company's to read, it is its participants'. The same guard admits the mailbox's Compose and
/// Reply, because a channel a participant may read is one they may write into -- whether the
/// message is *accepted* stays [`Channel::participant_access`]'s call on the inbound path, so the
/// mailbox and real mail are held to one rule rather than two.
pub(super) async fn load_viewable_channel(
    company_use_cases: &CompanyUseCases,
    channel_use_cases: &ChannelUseCases,
    viewer: &Viewer,
    company_id: Uuid,
    channel_id: Uuid,
) -> AppResult<(Company, Channel)> {
    let channel = channel_use_cases
        .get_readable_channel(viewer, company_id, channel_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Channel not found".into()))?;
    let company = company_use_cases
        .get_company(company_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Company not found".into()))?;

    Ok((company, channel))
}

/// A thread, but only when it really belongs to the channel the request claims.
pub(super) async fn load_channel_thread(
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

/// What each thread in a rendered page is doing, in one batched lookup.
///
/// Every column render goes through this rather than asking per row: a page is up to 50 threads,
/// and a query each would be 50 round trips for a badge most of them do not have.
async fn page_activity(
    thread_use_cases: &ThreadUseCases,
    threads: &[Thread],
) -> AppResult<HashMap<Uuid, ThreadActivity>> {
    let ids: Vec<Uuid> = threads.iter().map(|thread| thread.id).collect();
    thread_use_cases.thread_activity(&ids).await
}

/// What one thread is doing, for the strip under its messages.
async fn thread_activity(
    thread_use_cases: &ThreadUseCases,
    thread_id: Uuid,
) -> AppResult<Option<ThreadActivity>> {
    Ok(thread_use_cases
        .thread_activity(&[thread_id])
        .await?
        .get(&thread_id)
        .copied())
}

/// The face and name the agent side of a thread is drawn with.
///
/// A reply is stored with the *channel's* address as its sender, not the agent that wrote it, so
/// the agent is only unambiguous while the channel runs exactly one. With a stack of them the
/// messages stay on the address they were sent from -- better a plain address than the wrong face.
async fn channel_agent(
    agent_use_cases: &AgentUseCases,
    viewer: &Viewer,
    channel: &Channel,
) -> AppResult<Option<Agent>> {
    let [agent_id] = channel.agent_ids.as_deref().unwrap_or_default() else {
        return Ok(None);
    };

    agent_use_cases
        .get_readable_agent(viewer, channel.company_id, *agent_id)
        .await
}

async fn render_message_pane(
    thread_use_cases: &ThreadUseCases,
    company_id: Uuid,
    channel: &Channel,
    thread: &Thread,
    agent: Option<&Agent>,
    viewer_email: &EmailAddress,
) -> AppResult<String> {
    let messages = thread_use_cases.get_thread_history(thread.id).await?;
    Ok(pages::message_pane(&pages::MessagePane {
        company_id,
        channel,
        thread,
        messages: &messages,
        agent,
        viewer_email,
        activity: thread_activity(thread_use_cases, thread.id).await?,
    }))
}

/// GET /ui - The full mailbox shell for the selected company / channel / thread (Protected).
#[instrument(skip(
    company_use_cases,
    channel_use_cases,
    thread_use_cases,
    agent_use_cases,
    user_use_cases,
    config,
    user
))]
async fn mailbox_page(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    viewer: Viewer,
    Query(query): Query<MailboxQuery>,
) -> AppResult<Html<String>> {
    let account = load_account(&user_use_cases, user.id).await?;
    let account_email = EmailAddress::from(account.email.as_str());
    let mailbox_user = workspace_user(&account, &account_email, &config);

    let (companies, access) =
        load_readable_company(&company_use_cases, user.id, query.company_id).await?;
    let Some(access) = access else {
        return Ok(Html(pages::mailbox_no_company_page(&mailbox_user)));
    };
    let mailbox_user = mailbox_user.with_company_membership(access.membership);
    let company = access.company;

    let channels = channel_use_cases
        .list_readable_channels(&viewer, company.id)
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
            let agent = channel_agent(&agent_use_cases, &viewer, channel).await?;
            render_message_pane(
                &thread_use_cases,
                company.id,
                channel,
                thread,
                agent.as_ref(),
                &viewer.email,
            )
            .await?
        }
        (Some(_), None) => pages::empty_detail_pane(
            "Select a thread, or use New Thread to start a new one.",
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
        activity: &page_activity(&thread_use_cases, &thread_page.threads).await?,
        detail_html: &detail_html,
    })))
}

/// GET /ui/threads - The thread column for a channel, clearing the detail pane (Protected).
#[instrument(skip(channel_use_cases, thread_use_cases, viewer))]
async fn thread_column_fragment(
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    viewer: Viewer,
    Query(query): Query<ChannelQuery>,
) -> AppResult<Html<String>> {
    let channel = channel_use_cases
        .get_readable_channel(&viewer, query.company_id, query.channel_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Channel not found".into()))?;

    let page = load_thread_page(&thread_use_cases, channel.id, &ThreadListQuery::default()).await?;

    let activity = page_activity(&thread_use_cases, &page.threads).await?;
    let column = pages::thread_column(&pages::ThreadColumn {
        company_id: query.company_id,
        channel: &channel,
        threads: &page.threads,
        next_cursor: page.next_cursor.as_deref(),
        selected_thread_id: None,
        activity: &activity,
    });
    let detail = pages::empty_detail_pane(
        "Select a thread, or use New Thread to start a new one.",
        pages::FragmentSwap::OutOfBand,
    );
    let actions = pages::channel_actions(
        query.company_id,
        Some(&channel),
        pages::FragmentSwap::OutOfBand,
    );

    Ok(Html(format!("{column}{detail}{actions}")))
}

/// GET /ui/threads/list - One older page of threads, appended to the open column (Protected).
#[instrument(skip(channel_use_cases, thread_use_cases, viewer))]
async fn thread_page_fragment(
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    viewer: Viewer,
    Query(query): Query<ThreadPageQuery>,
) -> AppResult<Html<String>> {
    let channel = channel_use_cases
        .get_readable_channel(&viewer, query.company_id, query.channel_id)
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

    let activity = page_activity(&thread_use_cases, &page.threads).await?;
    Ok(Html(pages::thread_list_fragment(
        &pages::ThreadColumn {
            company_id: query.company_id,
            channel: &channel,
            threads: &page.threads,
            next_cursor: page.next_cursor.as_deref(),
            selected_thread_id: None,
            activity: &activity,
        },
        pages::FragmentSwap::OutOfBand,
    )))
}

/// GET /ui/messages - The messages of one thread (Protected).
#[instrument(skip(channel_use_cases, thread_use_cases, agent_use_cases, viewer))]
async fn message_pane_fragment(
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    viewer: Viewer,
    Query(query): Query<ThreadQuery>,
) -> AppResult<Html<String>> {
    let channel = channel_use_cases
        .get_readable_channel(&viewer, query.company_id, query.channel_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Channel not found".into()))?;
    let thread = load_channel_thread(&thread_use_cases, channel.id, query.thread_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Thread not found".into()))?;

    let agent = channel_agent(&agent_use_cases, &viewer, &channel).await?;

    Ok(Html(
        render_message_pane(
            &thread_use_cases,
            query.company_id,
            &channel,
            &thread,
            agent.as_ref(),
            &viewer.email,
        )
        .await?,
    ))
}

/// How far ahead of a reader one wake-up may catch them up. A reader further behind than this
/// gets the rest on the next event, rather than one query loading an unbounded backlog.
const STREAM_BATCH_LIMIT: usize = 50;

/// Where a live stream resumes from, in order of authority.
///
/// `Last-Event-ID` is the browser's own record of the last bubble it rendered, replayed
/// automatically when a dropped connection is re-established — so it beats anything the page was
/// rendered with. The `after` parameter is that page render's own high-water mark, which covers a
/// message landing between the render and the browser connecting. With neither, the thread streams
/// from its start, which is right for a thread that rendered empty.
fn resume_cursor<C: FromStr>(headers: &HeaderMap, after: Option<&str>) -> Option<C> {
    headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .or(after)
        .and_then(|raw| raw.parse().ok())
}

/// GET /ui/events - The open thread's messages as they are persisted (Protected).
///
/// Each event is one rendered chat bubble, appended to `#message-scroll` by htmx's SSE extension.
/// The `id:` on every event is that message's cursor, which is what makes reconnects resume
/// exactly where the reader left off.
#[instrument(skip(
    channel_use_cases,
    thread_use_cases,
    agent_use_cases,
    events,
    viewer,
    headers
))]
async fn thread_message_stream(
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    State(events): State<MailboxEvents>,
    viewer: Viewer,
    headers: HeaderMap,
    Query(query): Query<ThreadStreamQuery>,
) -> AppResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    // Authorize once, here: the stream that follows filters on ids alone, so nothing past this
    // point re-checks ownership.
    let channel = channel_use_cases
        .get_readable_channel(&viewer, query.company_id, query.channel_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Channel not found".into()))?;
    let thread = load_channel_thread(&thread_use_cases, channel.id, query.thread_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Thread not found".into()))?;

    // Resolved once, up here: the stream is authorized already and the channel's agent does not
    // change under it, so every bubble it renders can borrow this rather than re-query.
    let agent = channel_agent(&agent_use_cases, &viewer, &channel).await?;

    let mut cursor = resume_cursor(&headers, query.after.as_deref());
    let thread_id = thread.id;
    let mut wake_ups = Box::pin(thread_wake_ups(&events, "messages", thread_id));

    let stream = async_stream::stream! {
        // Both start true so a connect emits the reader's backlog and the current activity, rather
        // than leaving a freshly opened thread blank until something happens to change.
        let mut pending_messages = true;
        let mut pending_activity = true;

        loop {
            if pending_activity {
                pending_activity = false;
                match thread_activity(&thread_use_cases, thread_id).await {
                    // Activity is state, not an append: it carries no cursor and is re-read whole
                    // every time, which is also what makes it correct after a reconnect. An idle
                    // thread renders as an empty strip, clearing whatever was there.
                    Ok(activity) => {
                        yield Ok(Event::default()
                            .event("activity")
                            .data(pages::thread_activity_strip(activity)));
                    }
                    Err(error) => {
                        warn!(%error, %thread_id, "Thread activity query failed");
                        return;
                    }
                }
            }

            if pending_messages {
                match thread_use_cases
                    .get_thread_messages_after(thread_id, cursor, STREAM_BATCH_LIMIT)
                    .await
                {
                    Ok(messages) => {
                        for message in &messages {
                            cursor = Some(message.cursor());
                            yield Ok(Event::default()
                                .id(message.cursor().to_string())
                                .event("message")
                                .data(pages::message_bubble_chat(
                                    message,
                                    agent.as_ref(),
                                    Some(&viewer.email),
                                    pages::MessageScope {
                                        company_id: query.company_id,
                                        channel_id: channel.id,
                                    },
                                )));
                        }
                        // A full batch means the reader is still behind; keep going rather than
                        // waiting for an event that may never come.
                        pending_messages = messages.len() == STREAM_BATCH_LIMIT;
                        if pending_messages {
                            continue;
                        }
                    }
                    // Ending the stream makes the browser reconnect with `Last-Event-ID`, which
                    // resumes from the same cursor -- the right response to a transient failure.
                    Err(error) => {
                        warn!(%error, %thread_id, "Message stream query failed");
                        return;
                    }
                }
            }

            match wake_ups.next().await {
                Some(Wake::Event(MailboxEvent::MessageCommitted(_))) => pending_messages = true,
                Some(Wake::Event(MailboxEvent::ActivityChanged(_))) => pending_activity = true,
                // Something was missed and there is no telling what, so redo both.
                Some(Wake::Lagged) => {
                    pending_messages = true;
                    pending_activity = true;
                }
                None => return,
            }
        }
    };

    // Without the heartbeat an idle stream looks dead to the proxy in front of the app and is
    // dropped after a minute or two.
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// GET /ui/threads/events - The channel's thread column as threads are touched (Protected).
///
/// Each event is one rendered thread card, inserted at the top of `#thread-list`. A thread that
/// receives a message is bumped to the top of the column, so "insert at top, drop the stale copy"
/// is the whole reordering — and it leaves any older pages the reader loaded on demand in place,
/// which re-rendering the first page would not.
#[instrument(skip(channel_use_cases, thread_use_cases, events, viewer, headers))]
async fn thread_column_stream(
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(events): State<MailboxEvents>,
    viewer: Viewer,
    headers: HeaderMap,
    Query(query): Query<ChannelStreamQuery>,
) -> AppResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    // Authorize once, here: the stream that follows filters on ids alone.
    let channel = channel_use_cases
        .get_readable_channel(&viewer, query.company_id, query.channel_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Channel not found".into()))?;

    let company_id = query.company_id;
    let mut cursor = resume_cursor(&headers, query.after.as_deref());
    let channel_id = channel.id;
    let mut wake_ups = Box::pin(channel_wake_ups(&events, "threads", channel_id));

    let stream = async_stream::stream! {
        let mut pending_rows = true;
        // Threads whose badge is out of date. Kept as a set of ids rather than a flag because a
        // status change must redraw *only* that badge: re-sending the row would insert it at the
        // top of the column and move a thread for no reason a reader could see.
        let mut stale_badges: HashSet<Uuid> = HashSet::new();

        loop {
            if !stale_badges.is_empty() {
                let ids: Vec<Uuid> = stale_badges.drain().collect();
                match thread_use_cases.thread_activity(&ids).await {
                    Ok(activity) => {
                        for id in ids {
                            // A thread absent from the map has gone idle, and an empty payload is
                            // what clears its badge.
                            yield Ok(Event::default()
                                .event(pages::thread_activity_event(id))
                                .data(pages::thread_activity_mark(activity.get(&id).copied())));
                        }
                    }
                    Err(error) => {
                        warn!(%error, %channel_id, "Thread activity query failed");
                        return;
                    }
                }
            }

            if pending_rows {
                match thread_use_cases
                    .get_channel_threads_after(channel_id, cursor, STREAM_BATCH_LIMIT)
                    .await
                {
                    Ok(threads) => {
                        // A streamed row must arrive with its badge already on it, or a thread
                        // bumped mid-run would blink back to "idle" until its next status change.
                        let ids: Vec<Uuid> = threads.iter().map(|thread| thread.id).collect();
                        let activity = match thread_use_cases.thread_activity(&ids).await {
                            Ok(activity) => activity,
                            Err(error) => {
                                warn!(%error, %channel_id, "Thread activity query failed");
                                return;
                            }
                        };
                        // Who spoke last, so the client can mark an agent's reply and stay quiet
                        // about a person's message. Only streamed rows need it.
                        let last_roles = match thread_use_cases.thread_last_roles(&ids).await {
                            Ok(roles) => roles,
                            Err(error) => {
                                warn!(%error, %channel_id, "Thread last-role query failed");
                                return;
                            }
                        };

                        // Oldest first, so the newest is inserted last and ends up on top.
                        for thread in &threads {
                            cursor = Some(thread.cursor());
                            yield Ok(Event::default()
                                .id(thread.cursor().to_string())
                                .event("thread")
                                // Selection is a per-browser fact the server cannot know, so the
                                // row streams unselected and the client re-applies the highlight.
                                .data(pages::thread_row_fragment(
                                    company_id,
                                    &channel,
                                    thread,
                                    false,
                                    activity.get(&thread.id).copied(),
                                    last_roles.get(&thread.id).copied(),
                                )));
                        }
                        pending_rows = threads.len() == STREAM_BATCH_LIMIT;
                        if pending_rows {
                            continue;
                        }
                    }
                    Err(error) => {
                        warn!(%error, %channel_id, "Thread column stream query failed");
                        return;
                    }
                }
            }

            match wake_ups.next().await {
                Some(Wake::Event(MailboxEvent::MessageCommitted(_))) => pending_rows = true,
                Some(Wake::Event(MailboxEvent::ActivityChanged(scope))) => {
                    stale_badges.insert(scope.thread_id);
                }
                // What was missed is unknown, so redraw the rows and every badge the column is
                // likely to be showing rather than leave a stale spinner behind.
                Some(Wake::Lagged) => {
                    pending_rows = true;
                    match thread_use_cases
                        .list_channel_threads(channel_id, None, STREAM_BATCH_LIMIT)
                        .await
                    {
                        Ok(threads) => stale_badges.extend(threads.iter().map(|t| t.id)),
                        Err(error) => {
                            warn!(%error, %channel_id, "Thread column catch-up query failed");
                            return;
                        }
                    }
                }
                None => return,
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// GET /ui/compose - The new-thread form for the selected channel (Protected).
#[instrument(skip(company_use_cases, channel_use_cases, user_use_cases, config, viewer))]
async fn compose_form(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    viewer: Viewer,
    Query(query): Query<ChannelQuery>,
) -> AppResult<Html<String>> {
    let (company, channel) = load_viewable_channel(
        &company_use_cases,
        &channel_use_cases,
        &viewer,
        query.company_id,
        query.channel_id,
    )
    .await?;
    let sender_email = sender_email(&user_use_cases, viewer.user_id).await?;

    Ok(Html(pages::compose_pane(&pages::ComposePane {
        company_id: company.id,
        channel: &channel,
        channel_address: &channel.inbound_address(&company.slug, &config.app_domain_name),
        sender_email: &sender_email,
        subject: "",
        text_body: "",
        deliver: false,
        quiet: false,
        error: None,
    })))
}

/// POST /ui/compose - Start a new thread by feeding the composed message into the channel.
///
/// The message takes exactly the inbound path a real email would, so channel rules (participants,
/// spam, agents) apply unchanged; the toggle only decides whether the agent's reply is actually
/// delivered by email. That path is also why the guard here is the *read* guard: a participant who
/// is not the company's owner composes from the mailbox exactly as they would by email, and the
/// channel's own participant rules still decide whether the message lands.
#[instrument(skip(
    company_use_cases,
    channel_use_cases,
    thread_use_cases,
    agent_use_cases,
    user_use_cases,
    config,
    viewer,
    form
))]
async fn create_thread(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    viewer: Viewer,
    Form(form): Form<ComposeForm>,
) -> AppResult<Response> {
    let (company, channel) = load_viewable_channel(
        &company_use_cases,
        &channel_use_cases,
        &viewer,
        form.company_id,
        form.channel_id,
    )
    .await?;
    let sender_email = sender_email(&user_use_cases, viewer.user_id).await?;
    let address = channel.inbound_address(&company.slug, &config.app_domain_name);

    let subject = form.subject.unwrap_or_default();
    let text_body = form.text_body.unwrap_or_default();
    let deliver = delivery_requested(form.deliver.as_deref());
    let quiet = delivery_requested(form.quiet.as_deref());

    let compose_error = |message: String| {
        Html(pages::compose_pane(&pages::ComposePane {
            company_id: company.id,
            channel: &channel,
            channel_address: address.as_str(),
            sender_email: &sender_email,
            subject: &subject,
            text_body: &text_body,
            deliver,
            quiet,
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
        text: Some(message_body(&text_body, quiet)),
        ..Default::default()
    };
    let ingest = match thread_use_cases
        .queue_authenticated_inbound_for_agent(payload, delivery_mode(deliver))
        .await
    {
        Ok(ingest) => ingest,
        Err(err) => return Ok(compose_error(format!("Failed to send message: {err}"))),
    };

    let Some(thread) = ingest.thread else {
        let reason = ingest
            .reason
            .unwrap_or_else(|| "The channel rejected this message.".to_string());
        return Ok(compose_error(reason));
    };

    let agent = channel_agent(&agent_use_cases, &viewer, &channel).await?;

    sent_message_response(
        &thread_use_cases,
        company.id,
        &channel,
        &thread,
        agent.as_ref(),
        &viewer.email,
    )
    .await
}

/// GET /ui/reply - The new-message form for the open thread (Protected).
#[instrument(skip(
    company_use_cases,
    channel_use_cases,
    thread_use_cases,
    user_use_cases,
    config,
    viewer
))]
async fn reply_form(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    viewer: Viewer,
    Query(query): Query<ThreadQuery>,
) -> AppResult<Html<String>> {
    let (company, channel) = load_viewable_channel(
        &company_use_cases,
        &channel_use_cases,
        &viewer,
        query.company_id,
        query.channel_id,
    )
    .await?;
    let thread = load_channel_thread(&thread_use_cases, channel.id, query.thread_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Thread not found".into()))?;
    let sender_email = sender_email(&user_use_cases, viewer.user_id).await?;

    Ok(Html(pages::reply_pane(&pages::ReplyPane {
        company_id: company.id,
        channel: &channel,
        thread: &thread,
        channel_address: &channel.inbound_address(&company.slug, &config.app_domain_name),
        sender_email: &sender_email,
        text_body: "",
        deliver: false,
        quiet: false,
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
    agent_use_cases,
    user_use_cases,
    config,
    viewer,
    form
))]
async fn send_reply(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    viewer: Viewer,
    Form(form): Form<ReplyForm>,
) -> AppResult<Response> {
    let (company, channel) = load_viewable_channel(
        &company_use_cases,
        &channel_use_cases,
        &viewer,
        form.company_id,
        form.channel_id,
    )
    .await?;
    let thread = load_channel_thread(&thread_use_cases, channel.id, form.thread_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Thread not found".into()))?;
    let sender_email = sender_email(&user_use_cases, viewer.user_id).await?;
    let address = channel.inbound_address(&company.slug, &config.app_domain_name);

    let text_body = form.text_body.unwrap_or_default();
    let deliver = delivery_requested(form.deliver.as_deref());
    let quiet = delivery_requested(form.quiet.as_deref());

    let reply_error = |message: String| {
        Html(pages::reply_pane(&pages::ReplyPane {
            company_id: company.id,
            channel: &channel,
            thread: &thread,
            channel_address: address.as_str(),
            sender_email: &sender_email,
            text_body: &text_body,
            deliver,
            quiet,
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
        text: Some(message_body(&text_body, quiet)),
        headers: reply_headers(in_reply_to),
        ..Default::default()
    };

    let ingest = match thread_use_cases
        .queue_authenticated_inbound_for_agent(payload, delivery_mode(deliver))
        .await
    {
        Ok(ingest) => ingest,
        Err(err) => return Ok(reply_error(format!("Failed to send message: {err}"))),
    };

    let Some(sent_thread) = ingest.thread else {
        let reason = ingest
            .reason
            .unwrap_or_else(|| "The channel rejected this message.".to_string());
        return Ok(reply_error(reason));
    };

    let agent = channel_agent(&agent_use_cases, &viewer, &channel).await?;

    sent_message_response(
        &thread_use_cases,
        company.id,
        &channel,
        &sent_thread,
        agent.as_ref(),
        &viewer.email,
    )
    .await
}

/// What both send forms return: the thread's messages, with its column refreshed beside them.
async fn sent_message_response(
    thread_use_cases: &ThreadUseCases,
    company_id: Uuid,
    channel: &Channel,
    thread: &Thread,
    agent: Option<&Agent>,
    viewer_email: &EmailAddress,
) -> AppResult<Response> {
    let pane = render_message_pane(
        thread_use_cases,
        company_id,
        channel,
        thread,
        agent,
        viewer_email,
    )
    .await?;
    let page = load_thread_page(thread_use_cases, channel.id, &ThreadListQuery::default()).await?;
    let activity = page_activity(thread_use_cases, &page.threads).await?;
    let refreshed_list = pages::thread_list_oob(&pages::ThreadColumn {
        company_id,
        channel,
        threads: &page.threads,
        next_cursor: page.next_cursor.as_deref(),
        selected_thread_id: Some(thread.id),
        activity: &activity,
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
/// The signed-in account as every `/ui` shell renders it.
///
/// The address is passed in rather than derived here because the caller already needs it as an
/// [`EmailAddress`] of its own, and the shell borrows both for the length of the render.
/// The signed-in reader as the `/ui` chrome shows them.
///
/// Operator status is settled here rather than at each call site, so every workspace's account
/// menu offers the same set of entries -- the alternative is one page deciding differently from
/// the next about who is an operator.
pub(super) fn workspace_user<'a>(
    account: &'a User,
    account_email: &'a EmailAddress,
    config: &AppConfig,
) -> pages::MailboxUser<'a> {
    pages::MailboxUser {
        id: account.id,
        username: &account.username,
        email: account_email,
        avatar_url: account.avatar_url.as_ref(),
        is_operator: config.is_operator(account_email),
        company_membership: CompanyMembership::None,
    }
}

pub(super) async fn load_account(user_use_cases: &UserUseCases, user_id: Uuid) -> AppResult<User> {
    user_use_cases
        .get_user_by_id(user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("User not found".into()))
}

/// The signed-in user's own address — composed mail is always sent as them.
async fn sender_email(user_use_cases: &UserUseCases, user_id: Uuid) -> AppResult<String> {
    Ok(load_account(user_use_cases, user_id).await?.email)
}
