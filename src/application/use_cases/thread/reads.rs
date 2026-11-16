//! Viewer reads: load immutable stamps, then resolve current authority and remove inaccessible data.
use super::*;
use crate::entities::access_level::{AccessLevel, visible_to};

#[derive(Clone, Copy)]
pub struct InboxReadScope {
    pub company_id: Uuid,
    pub target: AddressTarget,
}
impl From<&Channel> for InboxReadScope {
    fn from(channel: &Channel) -> Self {
        Self {
            company_id: channel.company_id,
            target: AddressTarget::Desk(channel.id),
        }
    }
}
impl From<&super::ReadableInbox> for InboxReadScope {
    fn from(inbox: &super::ReadableInbox) -> Self {
        inbox.scope()
    }
}

#[derive(Clone, Copy)]
pub struct ThreadReadScope {
    pub inbox: InboxReadScope,
    pub thread_id: Uuid,
}
impl ThreadReadScope {
    pub fn new(channel: impl Into<InboxReadScope>, thread_id: Uuid) -> Self {
        Self {
            inbox: channel.into(),
            thread_id,
        }
    }
}

/// Remove data before it can reach a template, serializer, attachment lookup or diagnostics link.
pub(crate) fn project_message(message: &mut ThreadMessageView, viewer: AccessLevel) {
    if visible_to(viewer, message.access_level) {
        return;
    }
    message.restricted = true;
    message.subject.clear();
    message.body.clear();
    message.attachments.clear();
    message.response_contract = None;
    message.internal_note = None;
    message.run_ids.clear();
}

impl ThreadUseCases {
    async fn viewer_level(&self, viewer: &Viewer, company_id: Uuid) -> AppResult<AccessLevel> {
        // One fresh query returns joint role/level; a profile address is never an elevation.
        Ok(self
            .participant_persistence
            .access_context_for_user(company_id, viewer.user_id)
            .await?
            .map_or(AccessLevel::PUBLIC, |context| context.access_level))
    }

    pub(super) async fn require_readable_inbox(
        &self,
        viewer: &Viewer,
        scope: InboxReadScope,
    ) -> AppResult<()> {
        let allowed = match scope.target {
            AddressTarget::Desk(id) => {
                let readers = ChannelReaders {
                    companies: self.company_persistence.as_ref(),
                    channels: self.channel_persistence.as_ref(),
                    participants: self.participant_persistence.as_ref(),
                };
                readable_channel(&readers, viewer, scope.company_id, id)
                    .await?
                    .is_some()
            }
            AddressTarget::Principal(id) => {
                let principal = self
                    .participant_persistence
                    .principal_by_id(scope.company_id, id)
                    .await?;
                let context = self
                    .participant_persistence
                    .access_context_for_user(scope.company_id, viewer.user_id)
                    .await?;
                principal
                    .zip(context)
                    .is_some_and(|(principal, context)| principal.direct_viewer_access(context))
            }
        };
        if !allowed {
            return Err(AppError::NotFound("Inbox not found".into()));
        }
        Ok(())
    }

    pub(super) async fn project_thread_titles(
        &self,
        viewer: &Viewer,
        scope: InboxReadScope,
        mut threads: Vec<Thread>,
    ) -> AppResult<Vec<Thread>> {
        self.require_readable_inbox(viewer, scope).await?;
        let company_id = scope.company_id;
        let ids = threads.iter().map(|thread| thread.id).collect::<Vec<_>>();
        // A title may originate outside the bounded message window. Protect it conservatively
        // with the maximum immutable stamp, without hiding the thread or moving its cursor.
        let levels = self
            .thread_persistence
            .thread_access_levels(company_id, &ids)
            .await?;
        let level = self.viewer_level(viewer, company_id).await?;
        for thread in &mut threads {
            let stamp = levels
                .get(&thread.id)
                .copied()
                .ok_or_else(|| AppError::NotFound("Thread not found".into()))?;
            if !visible_to(level, stamp) {
                thread.subject = "Restricted message".into();
            }
        }
        Ok(threads)
    }

    pub async fn list_inbox_threads(
        &self,
        viewer: &Viewer,
        scope: InboxReadScope,
        before: Option<ThreadCursor>,
        limit: usize,
    ) -> AppResult<Vec<Thread>> {
        self.require_readable_inbox(viewer, scope).await?;
        let threads = self
            .thread_persistence
            .list_threads_by_target(scope.company_id, scope.target, before, limit)
            .await?;
        self.project_thread_titles(viewer, scope, threads).await
    }

    pub async fn inbox_threads_after(
        &self,
        viewer: &Viewer,
        scope: InboxReadScope,
        after: Option<ThreadCursor>,
        limit: usize,
    ) -> AppResult<Vec<Thread>> {
        self.require_readable_inbox(viewer, scope).await?;
        let threads = self
            .thread_persistence
            .list_threads_updated_after(scope.company_id, scope.target, after, limit)
            .await?;
        self.project_thread_titles(viewer, scope, threads).await
    }

    async fn project_messages(
        &self,
        viewer: &Viewer,
        scope: InboxReadScope,
        mut messages: Vec<ThreadMessageView>,
    ) -> AppResult<Vec<ThreadMessageView>> {
        self.require_readable_inbox(viewer, scope).await?;
        let company_id = scope.company_id;
        // This authority read follows ALL payload reads. Message stamps cannot change.
        let level = self.viewer_level(viewer, company_id).await?;
        for message in &mut messages {
            project_message(message, level);
        }
        Ok(messages)
    }

    /// Protocol metadata shown by simulation still belongs to the classified message.
    pub async fn viewer_email_reply_context(
        &self,
        viewer: &Viewer,
        scope: ThreadReadScope,
    ) -> AppResult<Option<EmailReplyContext>> {
        self.readable_thread(
            viewer,
            scope.inbox.company_id,
            scope.inbox.target,
            scope.thread_id,
        )
        .await?
        .ok_or_else(|| AppError::NotFound("Thread not found".into()))?;
        let Some(context) = self
            .thread_persistence
            .latest_email_reply_context(scope.thread_id)
            .await?
        else {
            return Ok(None);
        };
        let message = self
            .get_thread_message_view(viewer, scope, context.canonical_id)
            .await?;
        Ok(message
            .filter(|message| !message.restricted)
            .map(|_| context))
    }

    pub async fn get_thread_history(
        &self,
        viewer: &Viewer,
        scope: ThreadReadScope,
    ) -> AppResult<Vec<ThreadMessageView>> {
        self.readable_thread(
            viewer,
            scope.inbox.company_id,
            scope.inbox.target,
            scope.thread_id,
        )
        .await?
        .ok_or_else(|| AppError::NotFound("Thread not found".into()))?;
        let messages = self
            .thread_persistence
            .list_thread_message_views(scope.thread_id)
            .await?;
        self.project_messages(viewer, scope.inbox, messages).await
    }

    pub async fn get_thread_messages_after(
        &self,
        viewer: &Viewer,
        scope: ThreadReadScope,
        after: Option<MessageCursor>,
        limit: usize,
    ) -> AppResult<Vec<ThreadMessageView>> {
        self.readable_thread(
            viewer,
            scope.inbox.company_id,
            scope.inbox.target,
            scope.thread_id,
        )
        .await?
        .ok_or_else(|| AppError::NotFound("Thread not found".into()))?;
        let messages = self
            .thread_persistence
            .list_thread_message_views_after(scope.thread_id, after, limit)
            .await?;
        self.project_messages(viewer, scope.inbox, messages).await
    }

    pub async fn get_thread_message_view(
        &self,
        viewer: &Viewer,
        scope: ThreadReadScope,
        message_id: CanonicalMessageId,
    ) -> AppResult<Option<ThreadMessageView>> {
        self.readable_thread(
            viewer,
            scope.inbox.company_id,
            scope.inbox.target,
            scope.thread_id,
        )
        .await?
        .ok_or_else(|| AppError::NotFound("Thread not found".into()))?;
        let message = self
            .thread_persistence
            .get_thread_message_view(scope.thread_id, message_id)
            .await?;
        let projected = self
            .project_messages(viewer, scope.inbox, message.into_iter().collect())
            .await?;
        Ok(projected.into_iter().next())
    }

    /// Operational identifiers are visible only while the containing message is visible.
    pub async fn get_message_audit(
        &self,
        viewer: &Viewer,
        scope: ThreadReadScope,
        association_id: Uuid,
    ) -> AppResult<Option<MessageAuditView>> {
        self.readable_thread(
            viewer,
            scope.inbox.company_id,
            scope.inbox.target,
            scope.thread_id,
        )
        .await?
        .ok_or_else(|| AppError::NotFound("Thread not found".into()))?;
        let audit = self
            .thread_persistence
            .get_message_audit(scope.inbox.company_id, association_id)
            .await?
            .filter(|audit| {
                audit.thread_id == scope.thread_id && audit.target == scope.inbox.target
            });
        self.require_readable_inbox(viewer, scope.inbox).await?;
        let level = self.viewer_level(viewer, scope.inbox.company_id).await?;
        Ok(audit.filter(|audit| visible_to(level, audit.access_level)))
    }
}
