use super::*;
use crate::application::app_error::{AppError, AppResult};
use crate::application::use_cases::participant::PrincipalAccessPersistence;
use crate::application::workflow::{
    LifecycleAuthorizer, RelatedChannelId, RelatedThreadId, WorkflowChannelReader,
    WorkflowThreadReader,
};
use crate::domain::entities::{
    channel::Channel, company_member::CompanyMembership, participant::PrincipalAccessContext,
    thread::Thread, transport::QualifiedIdentity,
};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct State {
    pub membership: BTreeMap<(CompanyId, Uuid), CompanyMembership>,
    pub drafts: BTreeMap<WorkflowId, CompanyWorkflowDraft>,
    pub writes: usize,
    pub reader_error: bool,
    pub revoke_at_commit: bool,
}
#[derive(Clone, Default)]
pub struct Store(pub Arc<Mutex<State>>);
impl Store {
    pub fn grant(&self, company: CompanyId, actor: WorkflowActor, membership: CompanyMembership) {
        self.0
            .lock()
            .unwrap()
            .membership
            .insert((company, actor.user_id()), membership);
    }
}
#[async_trait]
impl PrincipalAccessPersistence for Store {
    async fn access_context_for_identity(
        &self,
        _: Uuid,
        _: &QualifiedIdentity,
    ) -> AppResult<PrincipalAccessContext> {
        panic!("not used")
    }
    async fn access_context_for_user(
        &self,
        company: Uuid,
        user: Uuid,
    ) -> AppResult<Option<PrincipalAccessContext>> {
        let state = self.0.lock().unwrap();
        if state.reader_error {
            return Err(AppError::Database("membership unavailable".into()));
        }
        Ok(state
            .membership
            .get(&(CompanyId::new(company), user))
            .map(|membership| PrincipalAccessContext {
                principal_id: None,
                membership: *membership,
            }))
    }
}
#[async_trait]
impl WorkflowChannelReader for Store {
    async fn workflow_channel(&self, _: RelatedChannelId) -> AppResult<Option<Channel>> {
        panic!("company association only")
    }
}
#[async_trait]
impl WorkflowThreadReader for Store {
    async fn workflow_thread(&self, _: RelatedThreadId) -> AppResult<Option<Thread>> {
        panic!("company association only")
    }
}
#[async_trait]
impl WorkflowDraftCopies for Store {
    async fn insert_copy(&self, copy: PreparedTemplateCopy) -> AppResult<()> {
        // Both competing calls reach the commit boundary before taking the lock.
        tokio::task::yield_now().await;
        let mut state = self.0.lock().unwrap();
        state.writes += 1;
        let key = (copy.draft().company(), copy.actor().user_id());
        if state.revoke_at_commit {
            state.membership.remove(&key);
        }
        if !state
            .membership
            .get(&key)
            .is_some_and(|m| m.manages_company_operations())
        {
            return Err(AppError::NotFound("workflow".into()));
        }
        if state.drafts.contains_key(&copy.draft().workflow()) {
            return Err(AppError::Conflict("workflow identity".into()));
        }
        state
            .drafts
            .insert(copy.draft().workflow(), copy.into_draft());
        Ok(())
    }
}
pub fn service(
    store: &Store,
) -> TemplateCopyService<LifecycleAuthorizer<Store, Store, Store>, Store, WorkflowSourceDecoder> {
    TemplateCopyService::new(
        LifecycleAuthorizer::new(store.clone(), store.clone(), store.clone()),
        store.clone(),
        WorkflowSourceDecoder,
    )
}
pub fn actor() -> WorkflowActor {
    WorkflowActor::authenticated(Uuid::from_u128(100)).unwrap()
}
pub fn company(n: u128) -> CompanyId {
    CompanyId::new(Uuid::from_u128(n))
}
pub fn workflow(n: u128) -> WorkflowId {
    WorkflowId::new(Uuid::from_u128(n))
}
pub fn request(company: CompanyId, revision: u64, destination: u128) -> CopyTemplateRequest {
    CopyTemplateRequest::new(
        company,
        actor(),
        TemplateId::parse("starter").unwrap(),
        TemplateRevision::new(revision).unwrap(),
        workflow(destination),
    )
    .unwrap()
}
pub fn offer(example: &registry::AuthoringExample, revision: u64) -> TemplateOffer<'_> {
    TemplateOffer {
        id: TemplateId::parse("starter").unwrap(),
        revision: TemplateRevision::new(revision).unwrap(),
        title: "Starter",
        description: "Company starter",
        source: &example.source,
        facts: &example.facts,
        dependencies: &example.dependencies,
    }
}
pub fn catalogue(example: &registry::AuthoringExample, revision: u64) -> TemplateCatalogue {
    TemplateCatalogue::build(&[offer(example, revision)], &WorkflowSourceDecoder).unwrap()
}
