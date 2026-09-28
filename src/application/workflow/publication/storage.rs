//! Bounded durable codec. Rebuilding runtime validators is allowed only when the
//! resulting compiled representation and publication identity match exactly.
use super::*;
use crate::application::app_error::{AppError, AppResult};
use compiler::SourceDecoder;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};

const FORMAT: &str = "workflow-bundle-storage-v1";

#[derive(Serialize)]
struct StoredBundle<'a> {
    format: &'static str,
    compiler_revision: u32,
    root: VersionId,
    versions: Vec<StoredVersion<'a>>,
}

#[derive(Serialize)]
struct StoredVersion<'a> {
    company: CompanyId,
    version: VersionId,
    source: &'a str,
    compiled: &'a Value,
    manifest: &'a Value,
    hash: &'a str,
    snapshots: &'a DependencySnapshots,
    children: Vec<VersionId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoadedBundle {
    format: String,
    compiler_revision: u32,
    root: VersionId,
    versions: Vec<LoadedVersion>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoadedVersion {
    company: CompanyId,
    version: VersionId,
    source: String,
    compiled: Value,
    manifest: Value,
    hash: String,
    snapshots: DependencySnapshots,
    children: Vec<VersionId>,
}

struct BoundedBytes(Vec<u8>);
impl Write for BoundedBytes {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_BUNDLE_BYTES.saturating_sub(self.0.len()) {
            return Err(io::Error::other(
                "workflow stored bundle exceeds byte budget",
            ));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn invalid(reason: &str) -> AppError {
    AppError::Database(format!("Invalid stored workflow bundle: {reason}"))
}

/// The complete deduplicated closure is bounded before serialization allocates.
/// Persist these bytes as JSON; do not serialize opaque runtime validator objects.
pub fn store_bundle(bundle: &PublishedBundle) -> AppResult<Vec<u8>> {
    let mut pending = vec![bundle];
    let mut visited = BTreeMap::<VersionId, &str>::new();
    let mut versions = Vec::new();
    while let Some(next) = pending.pop() {
        let version = next.compiled.graph().definition().version_id;
        if let Some(previous) = visited.get(&version) {
            if *previous != next.hash.as_str() {
                return Err(invalid("conflicting version identity"));
            }
            continue;
        }
        if visited.len() >= MAX_DEPENDENCIES {
            return Err(invalid("version closure exceeds budget"));
        }
        visited.insert(version, next.hash.as_str());
        versions.push(StoredVersion {
            company: next.company_id,
            version,
            source: next.compiled.source(),
            compiled: next.compiled.representation(),
            manifest: &next.manifest,
            hash: next.hash.as_str(),
            snapshots: &next.snapshots,
            children: next.children.keys().copied().collect(),
        });
        pending.extend(next.children.values().map(AsRef::as_ref));
    }
    let stored = StoredBundle {
        format: FORMAT,
        compiler_revision: compiler::SEMANTIC_REVISION,
        root: bundle.compiled.graph().definition().version_id,
        versions,
    };
    let mut bytes = BoundedBytes(Vec::new());
    serde_json::to_writer(&mut bytes, &stored).map_err(|_| invalid("serialization budget"))?;
    Ok(bytes.0)
}

/// Reject unknown formats, malformed facts, changed compilation/defaults, corrupted
/// hashes, foreign children and incomplete/cyclic closures. Never fall back to live facts.
pub fn restore_bundle(
    bytes: &[u8],
    decoder: &impl SourceDecoder,
) -> AppResult<Arc<PublishedBundle>> {
    if bytes.len() > MAX_BUNDLE_BYTES {
        return Err(invalid("byte budget"));
    }
    let stored: LoadedBundle =
        serde_json::from_slice(bytes).map_err(|_| invalid("format or value shape"))?;
    if stored.format != FORMAT
        || stored.compiler_revision != compiler::SEMANTIC_REVISION
        || stored.versions.is_empty()
        || stored.versions.len() > MAX_DEPENDENCIES
    {
        return Err(invalid("format or version count"));
    }
    let mut pending = BTreeMap::new();
    for version in stored.versions {
        if version.children.len() > MAX_DEPENDENCIES
            || pending.insert(version.version, version).is_some()
        {
            return Err(invalid("duplicate version or child budget"));
        }
    }
    let mut restored = BTreeMap::<VersionId, Arc<PublishedBundle>>::new();
    while !pending.is_empty() {
        let id = pending
            .iter()
            .find_map(|(id, version)| {
                version
                    .children
                    .iter()
                    .all(|child| restored.contains_key(child))
                    .then_some(*id)
            })
            .ok_or_else(|| invalid("missing or cyclic child"))?;
        let version = pending
            .remove(&id)
            .ok_or_else(|| invalid("version disappeared"))?;
        let bundle = restore_version(version, &restored, decoder)?;
        restored.insert(id, Arc::new(bundle));
    }
    let root = restored
        .remove(&stored.root)
        .ok_or_else(|| invalid("missing root"))?;
    // An unreachable injected version is not part of the canonical closure.
    let mut reachable = std::collections::BTreeSet::new();
    let mut pending = vec![root.as_ref()];
    while let Some(bundle) = pending.pop() {
        if reachable.insert(bundle.compiled.graph().definition().version_id) {
            pending.extend(bundle.children.values().map(AsRef::as_ref));
        }
    }
    if reachable.len() != restored.len() + 1 {
        return Err(invalid("unreachable version"));
    }
    Ok(root)
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;

fn restore_version(
    stored: LoadedVersion,
    restored: &BTreeMap<VersionId, Arc<PublishedBundle>>,
    decoder: &impl SourceDecoder,
) -> AppResult<PublishedBundle> {
    let decoded = decoder
        .decode(&stored.source)
        .map_err(|_| invalid("source"))?;
    let children = stored
        .children
        .iter()
        .map(|id| restored[id].clone())
        .collect();
    let bundle = freeze(
        decoded,
        stored.company,
        stored.version,
        stored.snapshots,
        children,
    )
    .map_err(|_| invalid("publication validation"))?;
    if bundle.compiled.representation() != &stored.compiled
        || bundle.manifest != stored.manifest
        || bundle.hash.as_str() != stored.hash
    {
        return Err(invalid("content identity or compiler version mismatch"));
    }
    Ok(bundle)
}
