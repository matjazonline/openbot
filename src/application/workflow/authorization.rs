use super::CompanyId;
use crate::application::app_error::{AppError, AppResult};
use crate::application::ports::channel::ChannelPersistence;
use crate::application::ports::participant::PrincipalAccessPersistence;
use crate::application::ports::thread::ThreadPersistence;
use crate::domain::entities::channel::Channel;
use crate::domain::entities::thread::Thread;
use crate::domain::entities::{
    address::AddressTarget, participant::PrincipalKind, transport::PrincipalId,
};
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
/// Thread identity includes its exact inbox target; the stored association is
/// reused for cancellation and participates in admission deduplication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RelatedAssociation {
    Company,
    Channel(RelatedChannelId),
    Principal(PrincipalId),
    Thread {
        target: AddressTarget,
        thread_id: RelatedThreadId,
    },
}

impl RelatedAssociation {
    pub fn inbox(target: AddressTarget) -> Self {
        match target {
            AddressTarget::Desk(id) => Self::Channel(RelatedChannelId::new(id)),
            AddressTarget::Principal(id) => Self::Principal(id),
        }
    }

    pub fn target(self) -> Option<AddressTarget> {
        match self {
            Self::Company => None,
            Self::Channel(id) => Some(AddressTarget::Desk(id.as_uuid())),
            Self::Principal(id) => Some(AddressTarget::Principal(id)),
            Self::Thread { target, .. } => Some(target),
        }
    }

    pub fn thread_id(self) -> Option<RelatedThreadId> {
        match self {
            Self::Thread { thread_id, .. } => Some(thread_id),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowOperation {
    Admit,
    Cancel,
    Retry,
    Reconcile,
    CopyTemplate,
    ManageDefinition,
    ManageBinding,
    /// Reading a definition, binding, run or decision through the workflow routes.
    Inspect,
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
    async fn workflow_thread(
        &self,
        company: CompanyId,
        id: RelatedThreadId,
    ) -> AppResult<Option<Thread>>;
}

#[async_trait]
impl<T: ThreadPersistence + Send + Sync> WorkflowThreadReader for T {
    async fn workflow_thread(
        &self,
        company: CompanyId,
        id: RelatedThreadId,
    ) -> AppResult<Option<Thread>> {
        self.get_thread_by_id(company.as_uuid(), id.as_uuid()).await
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
        operation: WorkflowOperation,
    ) -> AppResult<()> {
        let context = self
            .principals
            .access_context_for_user(company.as_uuid(), actor.user_id())
            .await?
            .ok_or_else(not_found)?;
        if !context.membership.manages_company_operations() {
            return Err(not_found());
        }
        let Some(target) = association.target() else {
            return Ok(());
        };
        match target {
            AddressTarget::Desk(id) => {
                let channel = self
                    .channels
                    .workflow_channel(RelatedChannelId::new(id))
                    .await?
                    .ok_or_else(not_found)?;
                if channel.id != id
                    || channel.company_id != company.as_uuid()
                    || !channel.viewer_access(context)
                {
                    return Err(not_found());
                }
            }
            AddressTarget::Principal(id) => {
                let principal = self
                    .principals
                    .principal_by_id(company.as_uuid(), id)
                    .await?
                    .ok_or_else(not_found)?;
                if principal.id != id
                    || principal.company_id != company.as_uuid()
                    || !principal.direct_viewer_access(context)
                {
                    return Err(not_found());
                }
                if operation == WorkflowOperation::ManageBinding
                    && principal.kind != PrincipalKind::Agent
                {
                    return Err(AppError::BadRequest(
                        "Workflow bindings require an agent's direct address".into(),
                    ));
                }
            }
        }
        if let RelatedAssociation::Thread { thread_id, .. } = association {
            let thread = self
                .threads
                .workflow_thread(company, thread_id)
                .await?
                .ok_or_else(not_found)?;
            if thread.id != thread_id.as_uuid() || !thread.belongs_to(company.as_uuid(), target) {
                return Err(not_found());
            }
        }
        Ok(())
    }
}

fn not_found() -> AppError {
    AppError::NotFound("workflow".into())
}
