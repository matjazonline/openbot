use super::*;
use crate::domain::workflow::WorkflowId;
use std::collections::BTreeSet;

pub(super) struct ChildFacts {
    pub contracts: Vec<registry::ChildContract>,
    pub graph: BTreeMap<WorkflowId, BTreeSet<WorkflowId>>,
    pub children: BTreeMap<VersionId, Arc<PublishedBundle>>,
    pub depth: usize,
}

pub(super) fn capture(
    company_id: CompanyId,
    parsed: &compiler::ParsedWorkflow,
    children: Vec<Arc<PublishedBundle>>,
) -> Result<ChildFacts, Diagnostic> {
    let fail = |id, code, reason| invalid_child(parsed, id, code, reason);
    if children.len() > MAX_DEPENDENCIES {
        return Err(fail(
            None,
            "publication.limit",
            "At most 256 child bundles are allowed",
        ));
    }
    let mut result = ChildFacts {
        contracts: Vec::new(),
        graph: BTreeMap::new(),
        children: BTreeMap::new(),
        depth: 1,
    };
    for child in children {
        let definition = child.compiled.graph().definition();
        let id = Some(definition.version_id);
        if child.company_id != company_id {
            return Err(fail(
                id,
                "publication.ownership",
                "Child belongs to another company",
            ));
        }
        if child.depth >= MAX_DEPENDENCY_DEPTH {
            return Err(fail(
                id,
                "publication.depth",
                "Child would exceed 32 bundle levels",
            ));
        }
        if result.children.contains_key(&definition.version_id) {
            return Err(fail(
                id,
                "publication.duplicate",
                "Child version supplied more than once",
            ));
        }
        result.depth = result.depth.max(child.depth + 1);
        result.contracts.push(registry::ChildContract {
            workflow_id: definition.workflow_id,
            version_id: definition.version_id,
            input_schema: definition.input_schema.clone().expect("compiled schema"),
            output_schema: definition.output_schema.clone().expect("compiled schema"),
        });
        result.children.insert(definition.version_id, child);
    }
    let expected: BTreeSet<_> = parsed
        .controls
        .values()
        .map(|c| c.child_version_id)
        .collect();
    for id in &expected {
        if !result.children.contains_key(id) {
            return Err(fail(
                Some(*id),
                "publication.missing",
                "Required child bundle is missing",
            ));
        }
    }
    for id in result.children.keys() {
        if !expected.contains(id) {
            return Err(fail(
                Some(*id),
                "publication.unused",
                "Child bundle has no declared call",
            ));
        }
    }
    result.graph = dependency_graph(parsed, &result.children)?;
    Ok(result)
}

fn invalid_child(
    parsed: &compiler::ParsedWorkflow,
    id: Option<VersionId>,
    code: &'static str,
    reason: &str,
) -> Diagnostic {
    let path = parsed
        .controls
        .iter()
        .find_map(|(step, control)| {
            (Some(control.child_version_id) == id)
                .then(|| format!("/steps/{step}/child_version_id"))
        })
        .unwrap_or_else(|| "/steps".into());
    let message = id.map_or_else(
        || reason.to_owned(),
        |id| format!("Child version {id}: {reason}"),
    );
    Diagnostic::at(
        code,
        &message,
        &path,
        compiler::input_span(&parsed.locations, &path),
    )
}

fn dependency_graph(
    parsed: &compiler::ParsedWorkflow,
    children: &BTreeMap<VersionId, Arc<PublishedBundle>>,
) -> Result<BTreeMap<WorkflowId, BTreeSet<WorkflowId>>, Diagnostic> {
    let fail = |id, code, reason| invalid_child(parsed, id, code, reason);
    let mut graph: BTreeMap<WorkflowId, BTreeSet<WorkflowId>> = BTreeMap::new();
    // Traverse unique immutable versions; reject conflicting content for one version.
    // Merge workflow-level edges for the compiler's recursion check.
    let mut versions = BTreeMap::new();
    let mut pending: Vec<_> = children.values().collect();
    while let Some(bundle) = pending.pop() {
        let definition = bundle.compiled.graph().definition();
        if definition.workflow_id == parsed.definition.workflow_id
            || definition.version_id == parsed.definition.version_id
        {
            return Err(fail(
                Some(definition.version_id),
                "publication.recursion",
                "Child closure repeats the root workflow or version identity",
            ));
        }
        if let Some(previous) = versions.insert(definition.version_id, &bundle.hash) {
            if previous != &bundle.hash {
                return Err(fail(
                    Some(definition.version_id),
                    "publication.conflict",
                    "Child version has conflicting frozen content",
                ));
            }
            continue;
        }
        if versions.len() > MAX_DEPENDENCIES {
            return Err(fail(
                Some(definition.version_id),
                "publication.limit",
                "Transitive closure exceeds 256 unique child versions",
            ));
        }
        graph.entry(definition.workflow_id).or_default().extend(
            bundle
                .children
                .values()
                .map(|c| c.compiled.graph().definition().workflow_id),
        );
        pending.extend(bundle.children.values());
    }
    graph.insert(
        parsed.definition.workflow_id,
        parsed
            .controls
            .values()
            .map(|c| c.child_workflow_id)
            .collect(),
    );
    Ok(graph)
}
