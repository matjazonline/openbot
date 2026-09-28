//! Pure construction of immutable publication content. The lifecycle command must
//! authorize and load company-owned snapshots through its ports before calling this
//! builder. These values are not grants, credentials, or a production publish command.
mod dependencies;
mod preflight;
mod storage;
mod types;
mod validation;

use super::compiler::{self, CompiledWorkflow, DecodedSource, Diagnostic, PreparedInputs};
use super::registry::{self, CatalogueFacts};
use crate::domain::workflow::{CompanyId, Context, StepId, VersionId};
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
        if matches!(
            definition.step_type.as_str(),
            "agent.run" | "decision.agent"
        ) {
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
        if let Some(profile) = inputs.value().get("capability_profile") {
            validation::validate_profile(
                profile,
                &self.snapshots,
                &self.compiled,
                &format!("/steps/{step}/with/capability_profile"),
            )?;
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
    preflight::validate(&snapshots, &children, &parsed)?;
    let child_facts = dependencies::capture(company_id, &parsed, children)?;
    let facts = CatalogueFacts {
        tools: snapshots.tools.iter().map(|t| t.contract.clone()).collect(),
        profiles: snapshots.profiles.clone(),
        children: child_facts.contracts,
    };
    let compiled = registry::compile(decoded, version_id, &facts, &child_facts.graph)?;
    validation::validate(company_id, &snapshots, &compiled)?;
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
        "format":"workflow-publication-v1", "company_id":company_id,
        "version_id":version_id, "compiled_hash":compiled.content_hash(),
        "dependencies":dependencies, "dependency_hashes":dependency_hashes,
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
