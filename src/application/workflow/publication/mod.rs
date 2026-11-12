//! Pure construction of immutable publication content. The lifecycle command must
//! authorize and load company-owned snapshots through its ports before calling this
//! builder. These values are not grants, credentials, or a production publish command.
mod capabilities;
mod dependencies;
mod preflight;
mod storage;
mod types;
mod validation;

use super::compiler::{self, CompiledWorkflow, DecodedSource, Diagnostic, PreparedInputs};
use super::registry::{self, CatalogueFacts};
use crate::domain::workflow::{CompanyId, Context, StepId, VersionId};
pub use capabilities::{ResolvedAgent, agent_reference_uses, agent_universe};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::Arc;
pub use storage::{restore_bundle, store_bundle};
pub use types::*;

pub const MAX_BUNDLE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_DEPENDENCIES: usize = 256;
pub const MAX_DEPENDENCY_DEPTH: usize = 32;

/// No mutable access or deserialization constructor: only checked construction can
/// produce a bundle. Child versions retain their complete immutable content by Arc.
pub struct PublishedBundle {
    company_id: CompanyId,
    compiled: CompiledWorkflow,
    snapshots: DependencySnapshots,
    children: BTreeMap<VersionId, Arc<PublishedBundle>>,
    manifest: Value,
    hash: ContentHash,
    depth: usize,
}

impl PublishedBundle {
    /// Requirements in pinned children need the same intake department as the root.
    pub fn first_role_reference(&self) -> Option<&RoleReference> {
        let mut pending = vec![self];
        let mut visited = std::collections::BTreeSet::new();
        while let Some(bundle) = pending.pop() {
            if !visited.insert(bundle.compiled.graph().definition().version_id) {
                continue;
            }
            if let Some(reference) = bundle.compiled.role_references().first() {
                return Some(reference);
            }
            pending.extend(bundle.children.values().map(AsRef::as_ref));
        }
        None
    }

    pub fn company_id(&self) -> CompanyId {
        self.company_id
    }
    pub fn compiled(&self) -> &CompiledWorkflow {
        &self.compiled
    }
    pub fn snapshots(&self) -> &DependencySnapshots {
        &self.snapshots
    }
    pub fn children(&self) -> &BTreeMap<VersionId, Arc<PublishedBundle>> {
        &self.children
    }
    pub fn manifest(&self) -> &Value {
        &self.manifest
    }
    pub fn content_hash(&self) -> &ContentHash {
        &self.hash
    }

    /// The distinct agents frozen in this bundle and in every child it pins (step children and
    /// workflow-tool pins), each child version visited once however many parents pin it. A
    /// display index (p09 gate P09-B1b-D1) for "used by" and the run list's agent filter;
    /// nothing decides or authorizes on it.
    pub fn frozen_agent_ids(&self) -> std::collections::BTreeSet<uuid::Uuid> {
        let mut agents = std::collections::BTreeSet::new();
        let mut visited = std::collections::BTreeSet::new();
        let mut pending = vec![self];
        while let Some(next) = pending.pop() {
            if !visited.insert(next.compiled.graph().definition().version_id) {
                continue;
            }
            agents.extend(
                next.snapshots
                    .agents
                    .iter()
                    .map(|agent| agent.agent_id.as_uuid()),
            );
            pending.extend(next.children.values().map(AsRef::as_ref));
        }
        agents
    }

    /// Admission/runtime must use this hook, not the compiler-only hook, to reject
    /// agent selectors outside the published finite catalogue. Current grants and
    /// revocation still require the runtime authorization boundary.
    pub fn prepare_step_inputs(
        &self,
        step: &StepId,
        context: &Context<'_>,
    ) -> Result<PreparedInputs, Diagnostic> {
        let inputs = self.compiled.prepare_step_inputs(step, context)?;
        let definition = &self.compiled.graph().definition().steps[step];
        if registry::ModelStep::of(definition.step_type.as_str()).is_some() {
            if let Some(reference) =
                self.compiled.role_references().iter().find(|reference| {
                    &reference.step == step && reference.mode.agent_mode().is_some()
                })
            {
                if inputs.value()["role"].as_str() != Some(reference.role.as_str()) {
                    return Err(validation::error(
                        &self.compiled,
                        &format!("/steps/{step}/with/role"),
                        "publication.role",
                        "Role selection does not match this frozen requirement",
                    ));
                }
                return Ok(inputs);
            }
            let selected = inputs.value()["agent"].as_str();
            if !self
                .snapshots
                .agents
                .iter()
                .any(|a| Some(a.key.as_str()) == selected)
            {
                return Err(validation::error(
                    &self.compiled,
                    &format!("/steps/{step}/with/agent"),
                    "publication.agent",
                    "Agent selection is not present in this frozen bundle",
                ));
            }
        }
        Ok(inputs)
    }
}

/// Inputs are captured values, not independently supplied compiler facts. Caller
/// ownership claims are checked for consistency here, not treated as authorization.
pub fn freeze(
    decoded: DecodedSource,
    company_id: CompanyId,
    version_id: VersionId,
    snapshots: DependencySnapshots,
    children: Vec<Arc<PublishedBundle>>,
) -> Result<PublishedBundle, Diagnostic> {
    let parsed = compiler::parse_workflow(&decoded, version_id)?;
    let references = capabilities::reference_uses(&parsed.definition).map_err(|reason| {
        Diagnostic::at(
            "publication.agent",
            &reason,
            "/steps",
            parsed.locations["/steps"],
        )
    })?;
    preflight::validate(&snapshots, &children, &parsed)?;
    let child_facts = dependencies::capture(company_id, &parsed, children, &snapshots.tools)?;
    let facts = CatalogueFacts {
        tools: snapshots.tools.iter().map(|t| t.contract.clone()).collect(),
        children: child_facts.contracts,
        tool_workflows: snapshots
            .tools
            .iter()
            .filter_map(|tool| match tool.kind() {
                ToolKind::Workflow(pin) => Some(pin.workflow_id),
                _ => None,
            })
            .collect(),
    };
    let compiled = registry::compile(decoded, version_id, &facts, &child_facts.graph)?;
    validation::validate(company_id, &snapshots, &compiled, &references)?;
    let manifest = manifest(
        company_id,
        version_id,
        &compiled,
        &snapshots,
        &child_facts.children,
    )?;
    let hash = hash(&manifest);
    Ok(PublishedBundle {
        company_id,
        compiled,
        snapshots,
        children: child_facts.children,
        manifest,
        hash,
        depth: child_facts.depth,
    })
}

fn manifest(
    company_id: CompanyId,
    version_id: VersionId,
    compiled: &CompiledWorkflow,
    snapshots: &DependencySnapshots,
    children: &BTreeMap<VersionId, Arc<PublishedBundle>>,
) -> Result<Value, Diagnostic> {
    let child_hashes: BTreeMap<_, _> = children.iter().map(|(id, b)| (id, &b.hash)).collect();
    let dependencies = serde_json::to_value(snapshots).expect("snapshot fields serialize");
    let dependency_hashes = dependencies
        .as_object()
        .expect("snapshot object")
        .iter()
        .map(|(kind, values)| {
            (
                kind.clone(),
                values
                    .as_array()
                    .expect("snapshot list")
                    .iter()
                    .map(hash)
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let manifest = json!({
        "format":"workflow-publication-v2", "company_id":company_id,
        "version_id":version_id, "compiled_hash":compiled.content_hash(),
        "dependencies":dependencies, "dependency_hashes":dependency_hashes,
        "roles":compiled.role_references(),
        "child_hashes":child_hashes,
        "resources":compiled.representation()["definition"]["resources"],
    });
    validation::check_manifest_size(&manifest, compiled)?;
    Ok(manifest)
}

fn hash(value: &Value) -> ContentHash {
    // serde_json uses sorted object keys (preserve_order is disabled in this crate).
    // Arrays intentionally preserve saved tool/skill selection order.
    let bytes = serde_json::to_vec(value).expect("JSON value serializes");
    ContentHash(format!("{:x}", Sha256::digest(bytes)))
}

#[cfg(test)]
mod tests;
