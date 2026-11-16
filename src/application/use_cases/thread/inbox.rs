//! Loaded policy for a thread's addressable target. Desks and direct inboxes share storage,
//! while their admission and participant policies remain owned by their domain entities.
use crate::entities::{
    address::AddressTarget,
    channel::{Channel, ParticipantAccess},
    participant::{Principal, PrincipalAccessContext},
    value_objects::ChannelSlug,
};

#[derive(Debug, Clone)]
pub enum Inbox {
    Desk(Box<Channel>),
    Direct {
        principal: Box<Principal>,
        slug: ChannelSlug,
    },
}

impl Inbox {
    pub fn target(&self) -> AddressTarget {
        match self {
            Self::Desk(desk) => AddressTarget::Desk(desk.id),
            Self::Direct { principal, .. } => AddressTarget::Principal(principal.id),
        }
    }

    pub fn slug(&self) -> &ChannelSlug {
        match self {
            Self::Desk(desk) => &desk.slug,
            Self::Direct { slug, .. } => slug,
        }
    }

    pub fn settings_revision(&self) -> i64 {
        match self {
            Self::Desk(desk) => desk.settings_revision,
            Self::Direct { principal, .. } => principal.settings_revision,
        }
    }

    pub fn participant_access(&self, context: PrincipalAccessContext) -> ParticipantAccess {
        match self {
            Self::Desk(desk) => desk.participant_access(context),
            Self::Direct { principal, .. } => principal.direct_access(context),
        }
    }

    pub fn desk(&self) -> Option<&Channel> {
        match self {
            Self::Desk(desk) => Some(desk),
            Self::Direct { .. } => None,
        }
    }
}

/// Current mailbox data. An archived direct target deliberately has no send address.
#[derive(Debug, Clone)]
pub enum ReadableInbox {
    Desk(Box<Channel>),
    Direct(Box<crate::application::ports::participant::DirectInbox>),
}

impl From<Channel> for ReadableInbox {
    fn from(channel: Channel) -> Self {
        Self::Desk(Box::new(channel))
    }
}
impl ReadableInbox {
    pub fn target(&self) -> AddressTarget {
        match self {
            Self::Desk(desk) => AddressTarget::Desk(desk.id),
            Self::Direct(direct) => AddressTarget::Principal(direct.principal.id),
        }
    }
    pub fn company_id(&self) -> uuid::Uuid {
        match self {
            Self::Desk(desk) => desk.company_id,
            Self::Direct(direct) => direct.principal.company_id,
        }
    }
    pub fn name(&self) -> &str {
        match self {
            Self::Desk(desk) => &desk.name,
            Self::Direct(direct) => &direct.principal.display_label,
        }
    }
    pub fn slug(&self) -> Option<&ChannelSlug> {
        match self {
            Self::Desk(desk) => Some(&desk.slug),
            Self::Direct(direct) => direct.slug.as_ref(),
        }
    }
    pub fn desk(&self) -> Option<&Channel> {
        match self {
            Self::Desk(desk) => Some(desk),
            Self::Direct(_) => None,
        }
    }
    pub fn scope(&self) -> super::InboxReadScope {
        super::InboxReadScope {
            company_id: self.company_id(),
            target: self.target(),
        }
    }
    pub fn thread_scope(&self, thread_id: uuid::Uuid) -> super::ThreadReadScope {
        super::ThreadReadScope {
            inbox: self.scope(),
            thread_id,
        }
    }
}

impl super::ThreadUseCases {
    pub async fn readable_inbox(
        &self,
        viewer: &crate::entities::user::Viewer,
        scope: super::InboxReadScope,
    ) -> crate::app_error::AppResult<ReadableInbox> {
        use crate::app_error::AppError;
        self.require_readable_inbox(viewer, scope).await?;
        let inbox = match scope.target {
            AddressTarget::Desk(id) => ReadableInbox::from(
                self.channel_persistence
                    .get_by_id(id)
                    .await?
                    .filter(|desk| desk.company_id == scope.company_id)
                    .ok_or_else(|| AppError::NotFound("Inbox not found".into()))?,
            ),
            AddressTarget::Principal(id) => {
                let principal = self
                    .participant_persistence
                    .principal_by_id(scope.company_id, id)
                    .await?
                    .ok_or_else(|| AppError::NotFound("Inbox not found".into()))?;
                let slug = self
                    .channel_persistence
                    .address_for_target(scope.company_id, scope.target)
                    .await?
                    .map(|address| address.slug);
                ReadableInbox::Direct(Box::new(
                    crate::application::ports::participant::DirectInbox { principal, slug },
                ))
            }
        };
        self.require_readable_inbox(viewer, scope).await?;
        Ok(inbox)
    }

    pub async fn direct_inboxes(
        &self,
        viewer: &crate::entities::user::Viewer,
        company_id: uuid::Uuid,
    ) -> crate::app_error::AppResult<Vec<ReadableInbox>> {
        let inboxes = self
            .participant_persistence
            .direct_inboxes(company_id, viewer.user_id, 201)
            .await?;
        let context = self
            .participant_persistence
            .access_context_for_user(company_id, viewer.user_id)
            .await?;
        Ok(inboxes
            .into_iter()
            .filter(|inbox| {
                inbox.principal.company_id == company_id
                    && context.is_some_and(|access| inbox.principal.direct_viewer_access(access))
            })
            .map(|inbox| ReadableInbox::Direct(Box::new(inbox)))
            .collect())
    }
}
