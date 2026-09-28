use super::*;
use crate::adapters::workflow_source::WorkflowSourceDecoder;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::Barrier;
use uuid::Uuid;

#[derive(Default)]
pub(super) struct Memory {
    pub drafts: HashMap<(CompanyId, WorkflowId), DraftState>,
    pub publications: HashMap<(CompanyId, IdempotencyKey), PublishedCommand>,
    pub versions: HashMap<(CompanyId, VersionId), Arc<PublishedBundle>>,
    pub bindings: HashMap<(CompanyId, WorkflowBindingId), BindingState>,
    pub revoked: bool,
    pub captures: usize,
    pub commits: usize,
    pub fail_capture: bool,
    pub fail_commit: bool,
    pub resources:
        HashMap<RuntimeResourceId, crate::application::workflow::binding::ResourceStatus>,
}

#[derive(Clone, Default)]
pub(super) struct Store {
    pub memory: Arc<Mutex<Memory>>,
    pub barrier: Option<Arc<Barrier>>,
    pub resume_commit: Option<Arc<Barrier>>,
}

impl Store {
    pub fn racing(&self) -> Self {
        Self {
            memory: self.memory.clone(),
            barrier: Some(Arc::new(Barrier::new(2))),
            resume_commit: None,
        }
    }
    fn authorized(state: &Memory) -> AppResult<()> {
        if state.revoked {
            return Err(missing());
        }
        Ok(())
    }
    fn commit(state: &Memory) -> AppResult<()> {
        Self::authorized(state)?;
        if state.fail_commit {
            return Err(conflict());
        }
        Ok(())
    }
}

#[async_trait]
impl WorkflowAuthorization for Store {
    async fn authorize(
        &self,
        company: CompanyId,
        _actor: WorkflowActor,
        association: RelatedAssociation,
        _operation: WorkflowOperation,
    ) -> AppResult<()> {
        Self::authorized(&self.memory.lock().unwrap())?;
        if company != target().company || association != RelatedAssociation::Company {
            return Err(missing());
        }
        Ok(())
    }
}

#[async_trait]
impl PublicationDirectory for Store {
    async fn capture(
        &self,
        _actor: WorkflowActor,
        _draft: &CompanyWorkflowDraft,
    ) -> AppResult<PublicationDependencies> {
        let mut state = self.memory.lock().unwrap();
        state.captures += 1;
        if state.fail_capture {
            return Err(AppError::BadRequest("directory unavailable".into()));
        }
        Ok(PublicationDependencies {
            snapshots: Default::default(),
            children: vec![],
        })
    }
}

#[async_trait]
impl ResourceDirectory for Store {
    async fn inspect(
        &self,
        _company: CompanyId,
        _actor: WorkflowActor,
        id: RuntimeResourceId,
    ) -> AppResult<Option<crate::application::workflow::binding::ResourceStatus>> {
        Ok(self.memory.lock().unwrap().resources.get(&id).cloned())
    }
}

#[async_trait]
impl WorkflowDefinitions for Store {
    async fn draft(
        &self,
        company: CompanyId,
        workflow: WorkflowId,
    ) -> AppResult<Option<DraftState>> {
        Ok(self
            .memory
            .lock()
            .unwrap()
            .drafts
            .get(&(company, workflow))
            .cloned())
    }
    async fn publication(
        &self,
        company: CompanyId,
        key: &IdempotencyKey,
    ) -> AppResult<Option<PublishedCommand>> {
        Ok(self
            .memory
            .lock()
            .unwrap()
            .publications
            .get(&(company, key.clone()))
            .cloned())
    }
    async fn selectable_version(
        &self,
        company: CompanyId,
        version: VersionId,
    ) -> AppResult<Option<Arc<PublishedBundle>>> {
        let state = self.memory.lock().unwrap();
        Ok(selectable(&state, company, version))
    }
    async fn save_draft(&self, command: PreparedDraft) -> AppResult<()> {
        if let Some(barrier) = &self.barrier {
            barrier.wait().await;
        }
        let mut memory = self.memory.lock().unwrap();
        Self::commit(&memory)?;
        let draft = &command.state().draft;
        let key = (draft.company(), draft.workflow());
        let existing = memory.drafts.get(&key);
        if existing.map(|s| s.draft.revision()) != command.expected()
            || existing.is_some_and(|s| s.archived)
        {
            return Err(conflict());
        }
        memory.drafts.insert(key, command.state().clone());
        memory.commits += 1;
        Ok(())
    }
    async fn publish(&self, command: PreparedPublication) -> AppResult<Arc<PublishedBundle>> {
        if let Some(barrier) = &self.barrier {
            barrier.wait().await;
        }
        let mut state = self.memory.lock().unwrap();
        Self::commit(&state)?;
        let command = command.command();
        let request = &command.request;
        let key = (request.target.company, request.key.clone());
        if let Some(existing) = state.publications.get(&key) {
            return if existing.request.equivalent(request) {
                Ok(existing.bundle.clone())
            } else {
                Err(conflict())
            };
        }
        let draft = state
            .drafts
            .get(&(request.target.company, request.target.workflow))
            .ok_or_else(missing)?;
        if draft.archived
            || draft.draft.revision() != request.expected
            || state
                .versions
                .contains_key(&(request.target.company, request.version))
        {
            return Err(conflict());
        }
        state.publications.insert(key, command.clone());
        state.versions.insert(
            (request.target.company, request.version),
            command.bundle.clone(),
        );
        state.commits += 1;
        Ok(command.bundle.clone())
    }
}

fn selectable(
    state: &Memory,
    company: CompanyId,
    version: VersionId,
) -> Option<Arc<PublishedBundle>> {
    let bundle = state.versions.get(&(company, version))?;
    let workflow = bundle.compiled().graph().definition().workflow_id;
    (!state.drafts.get(&(company, workflow))?.archived).then(|| bundle.clone())
}

#[async_trait]
impl BindingLifecycle for Store {
    async fn binding(
        &self,
        company: CompanyId,
        binding: WorkflowBindingId,
    ) -> AppResult<Option<BindingState>> {
        Ok(self
            .memory
            .lock()
            .unwrap()
            .bindings
            .get(&(company, binding))
            .cloned())
    }
    async fn save_binding(&self, command: PreparedBinding) -> AppResult<()> {
        if let Some(barrier) = &self.barrier {
            barrier.wait().await;
        }
        if let Some(barrier) = &self.resume_commit {
            barrier.wait().await;
        }
        let mut state = self.memory.lock().unwrap();
        Self::commit(&state)?;
        let new = command.state();
        let key = (new.configuration.company_id(), new.configuration.id());
        let existing = state.bindings.get(&key);
        if existing.map(|s| s.revision) != command.expected()
            || existing.is_some_and(|s| s.association != new.association)
        {
            return Err(conflict());
        }
        let reconfigured =
            existing.is_none_or(|s| s.configuration.revision() != new.configuration.revision());
        let version = new
            .configuration
            .bundle()
            .compiled()
            .graph()
            .definition()
            .version_id;
        if (new.active || reconfigured) && selectable(&state, key.0, version).is_none() {
            return Err(conflict());
        }
        if new.active || reconfigured {
            for requirement in &new
                .configuration
                .bundle()
                .compiled()
                .graph()
                .definition()
                .resources
            {
                let id = new.configuration.resources()[&requirement.slot];
                let status = state.resources.get(&id).ok_or_else(missing)?;
                crate::application::workflow::binding::validate_resource(
                    key.0,
                    id,
                    requirement,
                    status,
                )?;
            }
        }
        state.bindings.insert(key, new.clone());
        state.commits += 1;
        Ok(())
    }
}

pub(super) fn target() -> DefinitionTarget {
    DefinitionTarget {
        company: CompanyId::new(Uuid::from_u128(10)),
        actor: WorkflowActor::authenticated(Uuid::from_u128(11)).unwrap(),
        workflow: WorkflowId::new(Uuid::from_u128(1)),
    }
}
pub(super) fn source() -> String {
    crate::application::workflow::registry::example("data.map")
        .unwrap()
        .source
}
pub(super) fn draft(source: String, expected: Option<DraftRevision>) -> SaveDraft {
    SaveDraft {
        target: target(),
        expected,
        content: DraftContent {
            title: "Draft".into(),
            description: "Description".into(),
            source,
        },
    }
}
pub(super) fn publish() -> PublishDraft {
    PublishDraft {
        target: target(),
        expected: DraftRevision::new(1).unwrap(),
        version: VersionId::new(Uuid::from_u128(2)),
        key: IdempotencyKey::parse("publish").unwrap(),
    }
}
pub(super) fn binding_target() -> BindingTarget {
    BindingTarget {
        company: target().company,
        actor: target().actor,
        binding: WorkflowBindingId::new(Uuid::from_u128(20)),
    }
}
pub(super) fn configuration(expected: Option<BindingStateRevision>) -> ConfigureBinding {
    ConfigureBinding {
        target: binding_target(),
        expected,
        association: RelatedAssociation::Company,
        version: publish().version,
        params: serde_json::json!({}),
        resources: Default::default(),
    }
}
pub(super) fn definitions(
    store: &Store,
) -> DefinitionService<Store, Store, WorkflowSourceDecoder, Store> {
    DefinitionService::new(
        store.clone(),
        store.clone(),
        WorkflowSourceDecoder,
        store.clone(),
    )
}
pub(super) fn bindings(store: &Store) -> BindingService<Store, Store, Store, Store> {
    BindingService::new(store.clone(), store.clone(), store.clone(), store.clone())
}
pub(super) async fn published(store: &Store) -> Arc<PublishedBundle> {
    definitions(store)
        .save(draft(source(), None))
        .await
        .unwrap();
    definitions(store).publish(publish()).await.unwrap()
}
