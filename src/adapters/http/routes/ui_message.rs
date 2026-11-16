//! Exact source-message destinations are independent of the bounded live conversation window.
use super::*;

#[derive(Debug, Clone, Deserialize)]
pub(super) struct MessageQuery {
    pub company_id: Uuid,
    #[serde(flatten)]
    pub inbox: super::InboxSelection,
    pub thread_id: Uuid,
    pub message_id: Uuid,
}

pub(super) async fn page(
    State(companies): State<Arc<CompanyUseCases>>,
    State(_channels): State<Arc<ChannelUseCases>>,
    State(threads): State<Arc<ThreadUseCases>>,
    State(activity): State<Arc<dyn ThreadActivityReader>>,
    viewer: Viewer,
    Query(query): Query<MessageQuery>,
) -> AppResult<Html<String>> {
    let (_, channel) = super::inbox::load_viewable_inbox(
        &companies,
        &threads,
        &viewer,
        query.company_id,
        query.inbox,
    )
    .await?;
    let thread = threads
        .readable_thread(
            &viewer,
            channel.company_id(),
            channel.target(),
            query.thread_id,
        )
        .await?
        .ok_or_else(|| AppError::NotFound("Thread not found".into()))?;
    let message = threads
        .get_thread_message_view(
            &viewer,
            crate::use_cases::thread::ThreadReadScope::new(&channel, thread.id),
            CanonicalMessageId::new(query.message_id),
        )
        .await?
        .ok_or_else(|| AppError::NotFound("Message not found in this thread".into()))?;
    let inspectable = activity
        .inspectable_runs(query.company_id, viewer.user_id, &message.run_ids)
        .await?;
    threads.readable_inbox(&viewer, channel.scope()).await?;
    Ok(Html(pages::workflow_source_message_page(
        &message,
        &viewer.email,
        pages::MessageScope {
            company_id: query.company_id,
            target: channel.target(),
        },
        pages::RunLinkAccess::Authorized(&inspectable),
    )))
}
