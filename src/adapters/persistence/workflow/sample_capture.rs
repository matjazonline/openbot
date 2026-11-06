//! Ephemeral canonical draft capture. This adapter writes no production records.
use super::*;
use crate::application::agent::dependency::{
    DependencyNode as Node, DependencyWalker, agent_edges, workflow_edges,
};
use crate::application::agent::{
    AgentPublicationScope, MAX_AGENT_DEPENDENCIES, MAX_AGENT_MANIFEST_BYTES,
    ResolvedAgentDefinition,
};
use crate::application::workflow::compiler::SourceDecoder;
use crate::application::workflow::lifecycle::{
    DefinitionTarget, LifecycleError, check_draft, compile_bundle,
};
use crate::application::workflow::sample::{MAX_SAMPLE_BYTES, PreparedSample, SampleRequest};
use crate::application::workflow::sample_capture::SampleDefinitionCapture;
use crate::entities::agent_capability::AgentId;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

#[cfg(test)]
#[path = "sample_capture_bounds_tests.rs"]
mod bounds;

pub struct SampleCapture {
    pub persistence: PostgresPersistence,
    pub http: Vec<http::HttpPublicationSelection>,
    pub mcp: Vec<mcp::McpPublicationSelection>,
    pub refused_kinds: &'static [&'static str],
}
fn failure(error: LifecycleError) -> AppError {
    match error {
        LifecycleError::Application(error) => error,
        LifecycleError::Validation(error) => AppError::BadRequest(error.message.to_string()),
    }
}
fn timeout() -> AppError {
    AppError::Timeout("Sample definition capture exceeded five seconds".into())
}
#[async_trait]
impl SampleDefinitionCapture for SampleCapture {
    async fn capture(
        &self,
        target: DefinitionTarget,
        expected: DraftRevision,
        request: SampleRequest,
    ) -> AppResult<PreparedSample> {
        if !request.agents.is_empty()
            || serde_json::to_vec(&request).map_err(|_| invalid())?.len() > MAX_SAMPLE_BYTES
        {
            return Err(AppError::BadRequest("Invalid sample capture input".into()));
        }
        tokio::time::timeout(Duration::from_secs(5), async {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut tx = self.persistence.pool().begin().await?;
            authorize_on(&mut tx, target).await?;
            let root = self.root_on(&mut tx, target, expected).await?;
            let mut closure = Closure::new(target, deadline);
            closure.cache(root.clone())?;
            closure.capture_on(&mut tx, &root).await?;
            let mut request = request;
            request.agents = closure.agents.into_values().collect();
            let prepared =
                PreparedSample::new(root, request, closure.workflows.into_values().collect())?;
            if Instant::now() >= deadline {
                return Err(timeout());
            }
            tx.commit().await?;
            Ok(prepared)
        })
        .await
        .map_err(|_| timeout())?
    }
    async fn authorize_release(&self, target: DefinitionTarget) -> AppResult<()> {
        let mut tx = self.persistence.pool().begin().await?;
        authorize_on(&mut tx, target).await?;
        definitions::read_draft(&mut tx, target.company, target.workflow)
            .await?
            .ok_or_else(missing)?;
        tx.commit().await?;
        Ok(())
    }
}
async fn authorize_on(
    tx: &mut Transaction<'_, Postgres>,
    target: DefinitionTarget,
) -> AppResult<()> {
    sqlx::query("SELECT set_config('lock_timeout','1000ms',true),set_config('statement_timeout','5000ms',true)").execute(&mut **tx).await?;
    sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE id=$1 FOR SHARE")
        .bind(target.actor.user_id())
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(missing)?;
    authority::authorize_company(tx, target.company, target.actor).await?;
    Ok(())
}
impl SampleCapture {
    async fn root_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        target: DefinitionTarget,
        expected: DraftRevision,
    ) -> AppResult<Arc<PublishedBundle>> {
        let state = definitions::read_draft(tx, target.company, target.workflow)
            .await?
            .ok_or_else(missing)?;
        check_draft(&state, target, expected)?;
        let decoder = crate::adapters::workflow_source::WorkflowSourceDecoder;
        let decoded = decoder
            .decode(state.draft.source())
            .map_err(|error| AppError::BadRequest(error.message.to_string()))?;
        let parsed = crate::application::workflow::compiler::parse_workflow(
            &decoded,
            VersionId::new(Uuid::nil()),
        )
        .map_err(|error| AppError::BadRequest(error.message.to_string()))?;
        let mut captured = message::message_builtins(target.company)?;
        // Samples need the metadata used by their STEP consumers. Unused global
        // message descriptors must not consume the declared agent universe budget.
        captured.snapshots.tools.retain(|tool| {
            parsed
                .definition
                .steps
                .values()
                .any(|step| step.step_type.as_str() == tool.contract.name.as_str())
        });
        captured
            .snapshots
            .tools
            .extend(http::capture_publication_on(tx, target.company, &self.http).await?);
        captured
            .snapshots
            .tools
            .extend(mcp::capture_publication_on(tx, target.company, &self.mcp).await?);
        agent_publication::capture_sample_agents_on(tx, &state.draft, &mut captured).await?;
        let captured = child_publication::capture_on(tx, &state.draft, captured).await?;
        Ok(Arc::new(
            compile_bundle(
                decoded,
                &state.draft,
                VersionId::new(Uuid::nil()),
                captured,
                self.refused_kinds,
            )
            .map_err(failure)?,
        ))
    }
}
struct Closure {
    target: DefinitionTarget,
    deadline: Instant,
    agents: BTreeMap<AgentId, ResolvedAgentDefinition>,
    workflows: BTreeMap<VersionId, Arc<PublishedBundle>>,
    workflow_bytes: usize,
    agent_bytes: usize,
}
impl Closure {
    fn new(target: DefinitionTarget, deadline: Instant) -> Self {
        Self {
            target,
            deadline,
            agents: BTreeMap::new(),
            workflows: BTreeMap::new(),
            workflow_bytes: 0,
            agent_bytes: 0,
        }
    }
    fn capacity(&self) -> AppResult<()> {
        if Instant::now() >= self.deadline {
            return Err(timeout());
        }
        if self.agents.len() + self.workflows.len() >= MAX_AGENT_DEPENDENCIES {
            return Err(AppError::BadRequest(
                "Too many sample dependency selections".into(),
            ));
        }
        Ok(())
    }
    fn cache(&mut self, root: Arc<PublishedBundle>) -> AppResult<()> {
        let mut pending = vec![root];
        while let Some(bundle) = pending.pop() {
            let d = bundle.compiled().graph().definition();
            if bundle.company_id() != self.target.company {
                return Err(missing());
            }
            if let Some(prior) = self.workflows.get(&d.version_id) {
                if prior.content_hash() != bundle.content_hash()
                    || prior.compiled().graph().definition().workflow_id != d.workflow_id
                {
                    return Err(invalid());
                }
                continue;
            }
            self.capacity()?;
            self.workflow_bytes = self
                .workflow_bytes
                .saturating_add(store_bundle(&bundle)?.len());
            if self.workflow_bytes > MAX_AGENT_MANIFEST_BYTES {
                return Err(AppError::BadRequest(
                    "Sample workflow definitions exceed 4 MiB".into(),
                ));
            }
            pending.extend(bundle.children().values().cloned());
            self.workflows.insert(d.version_id, bundle);
        }
        Ok(())
    }
    async fn capture_on(
        &mut self,
        tx: &mut Transaction<'_, Postgres>,
        root: &PublishedBundle,
    ) -> AppResult<()> {
        let d = root.compiled().graph().definition();
        let mut walk = DependencyWalker::new(Node::Workflow(d.workflow_id, d.version_id));
        while let Some(node) = walk.next()? {
            if Instant::now() >= self.deadline {
                return Err(timeout());
            }
            let children = match node {
                Node::Workflow(workflow, version) => {
                    let bundle = self.workflow_on(tx, workflow, version).await?;
                    workflow_edges(&bundle)
                }
                Node::Agent(agent, mode) => {
                    self.agent_on(tx, agent).await?;
                    agent_edges(self.agents.get(&agent).ok_or_else(invalid)?, mode)
                }
            };
            walk.supply(node, children)?;
        }
        Ok(())
    }
    async fn workflow_on(
        &mut self,
        tx: &mut Transaction<'_, Postgres>,
        workflow: WorkflowId,
        version: VersionId,
    ) -> AppResult<Arc<PublishedBundle>> {
        if !self.workflows.contains_key(&version) {
            self.capacity()?;
            let remaining = MAX_AGENT_MANIFEST_BYTES.saturating_sub(self.workflow_bytes);
            let bytes:Vec<u8>=sqlx::query_scalar("SELECT bundle FROM workflow_versions WHERE company_id=$1 AND workflow_id=$2 AND id=$3 AND octet_length(bundle)<=$4 FOR SHARE")
                .bind(self.target.company.as_uuid()).bind(workflow.as_uuid()).bind(version.as_uuid()).bind(remaining as i32).fetch_optional(&mut **tx).await?.ok_or_else(missing)?;
            self.cache(restore_bundle(
                &bytes,
                &crate::adapters::workflow_source::WorkflowSourceDecoder,
            )?)?;
        }
        let bundle = self.workflows.get(&version).ok_or_else(missing)?;
        if bundle.compiled().graph().definition().workflow_id != workflow {
            return Err(missing());
        }
        Ok(bundle.clone())
    }
    async fn agent_on(
        &mut self,
        tx: &mut Transaction<'_, Postgres>,
        agent: AgentId,
    ) -> AppResult<()> {
        if self.agents.contains_key(&agent) {
            return Ok(());
        }
        self.capacity()?;
        let scope = AgentPublicationScope {
            company: self.target.company,
            actor: self.target.actor,
            agent,
        };
        let preview =
            crate::adapters::persistence::agent_publication::LockedAgentPublication::lock(
                tx, scope,
            )
            .await?
            .preview()
            .await?;
        self.agent_bytes = self
            .agent_bytes
            .saturating_add(preview.definition.canonical_content()?.len());
        if self.agent_bytes > MAX_SAMPLE_BYTES {
            return Err(AppError::BadRequest(
                "Sample agent definitions exceed 256 KiB".into(),
            ));
        }
        self.agents.insert(agent, preview.definition);
        Ok(())
    }
}
