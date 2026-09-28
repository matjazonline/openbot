use super::CompanyId;
use crate::application::app_error::{AppError, AppResult};
use crate::application::use_cases::channel::ChannelPersistence;
use crate::application::use_cases::participant::PrincipalAccessPersistence;
use crate::application::use_cases::thread::ThreadPersistence;
use crate::domain::entities::channel::Channel;
use crate::domain::entities::thread::Thread;
use async_trait::async_trait;
use uuid::Uuid;

/// An authenticated account identity supplied by a trusted ingress boundary.
/// A nil or caller-supplied membership is never accepted as authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkflowActor {
    user_id: Uuid,
}

impl WorkflowActor {
    pub fn authenticated(user_id: Uuid) -> AppResult<Self> {
        if user_id.is_nil() {
            return Err(AppError::BadRequest(
                "missing authenticated workflow actor".into(),
            ));
        }
        Ok(Self { user_id })
    }

    pub fn user_id(self) -> Uuid {
        self.user_id
    }
}

macro_rules! related_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name(Uuid);
        impl $name {
            pub fn new(id: Uuid) -> Self {
                Self(id)
            }
            pub fn as_uuid(self) -> Uuid {
                self.0
            }
        }
    };
}
related_id!(RelatedChannelId);
related_id!(RelatedThreadId);

/// Related visibility is checked independently from company management access.
/// Thread identity includes its declared channel; the stored association is
/// reused for cancellation and participates in admission deduplication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelatedAssociation {
    Company,
    Channel(RelatedChannelId),
    Thread {
        channel_id: RelatedChannelId,
        thread_id: RelatedThreadId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowOperation {
    Admit,
    Cancel,
    CopyTemplate,
    ManageDefinition,
    ManageBinding,
}

/// Required lifecycle authorization for authenticated workflow lifecycle and authoring calls.
/// The trusted ingress supplies the actor's user ID; current membership is
/// loaded from persistence. Owners and admins may manage company workflows;
/// members and outsiders receive a non-disclosing `NotFound`. Related channel
/// visibility is checked independently, including allowlist grants and the
/// company owner's exception. A thread must belong to the visible channel.
/// Missing, foreign, or inaccessible related records also return `NotFound`;
/// reader infrastructure errors propagate unchanged.
///
/// Reader results are untrusted until their returned IDs, company, visibility,
/// and thread/channel relationship have been verified. Neither prompts nor
/// declared resource requirements grant authority. Every replay calls this
/// again against current membership. This preflight is not durable protection
/// from revocation races: later persistence/effect commits must recheck current
/// authority transactionally where required. Resource and execution/effect
/// authorization belong to later phases.
#[async_trait]
pub trait WorkflowAuthorization: Send + Sync {
    async fn authorize(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        association: RelatedAssociation,
        operation: WorkflowOperation,
    ) -> AppResult<()>;
}

#[async_trait]
pub trait WorkflowChannelReader: Send + Sync {
    async fn workflow_channel(&self, id: RelatedChannelId) -> AppResult<Option<Channel>>;
}

#[async_trait]
impl<T: ChannelPersistence + Send + Sync> WorkflowChannelReader for T {
    async fn workflow_channel(&self, id: RelatedChannelId) -> AppResult<Option<Channel>> {
        self.get_by_id(id.as_uuid()).await
    }
}

#[async_trait]
pub trait WorkflowThreadReader: Send + Sync {
    async fn workflow_thread(&self, id: RelatedThreadId) -> AppResult<Option<Thread>>;
}

#[async_trait]
impl<T: ThreadPersistence + Send + Sync> WorkflowThreadReader for T {
    async fn workflow_thread(&self, id: RelatedThreadId) -> AppResult<Option<Thread>> {
        self.get_thread_by_id(id.as_uuid()).await
    }
}

pub struct LifecycleAuthorizer<P, C, T> {
    principals: P,
    channels: C,
    threads: T,
}

impl<P, C, T> LifecycleAuthorizer<P, C, T> {
    pub fn new(principals: P, channels: C, threads: T) -> Self {
        Self {
            principals,
            channels,
            threads,
        }
    }
}

#[async_trait]
impl<P, C, T> WorkflowAuthorization for LifecycleAuthorizer<P, C, T>
where
    P: PrincipalAccessPersistence,
    C: WorkflowChannelReader,
    T: WorkflowThreadReader,
{
    async fn authorize(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        association: RelatedAssociation,
        _operation: WorkflowOperation,
    ) -> AppResult<()> {
        let context = self
            .principals
            .access_context_for_user(company.as_uuid(), actor.user_id())
            .await?
            .ok_or_else(not_found)?;
        if !context.membership.manages_company_operations() {
            return Err(not_found());
        }
        let channel_id = match association {
            RelatedAssociation::Company => return Ok(()),
            RelatedAssociation::Channel(id) => id,
            RelatedAssociation::Thread { channel_id, .. } => channel_id,
        };
        let channel = self
            .channels
            .workflow_channel(channel_id)
            .await?
            .ok_or_else(not_found)?;
        if channel.id != channel_id.as_uuid()
            || channel.company_id != company.as_uuid()
            || !channel.viewer_access(context)
        {
            return Err(not_found());
        }
        if let RelatedAssociation::Thread { thread_id, .. } = association {
            let thread = self
                .threads
                .workflow_thread(thread_id)
                .await?
                .ok_or_else(not_found)?;
            if thread.id != thread_id.as_uuid() || thread.channel_id != channel_id.as_uuid() {
                return Err(not_found());
            }
        }
        Ok(())
    }
}

fn not_found() -> AppError {
    AppError::NotFound("workflow".into())
}
