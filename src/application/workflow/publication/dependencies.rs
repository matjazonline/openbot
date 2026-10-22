use super::*;
use crate::domain::workflow::WorkflowId;
use std::collections::BTreeSet;

pub(super) struct ChildFacts {
    pub contracts: Vec<registry::ChildContract>,
    pub graph: BTreeMap<WorkflowId, BTreeSet<WorkflowId>>,
    pub children: BTreeMap<VersionId, Arc<PublishedBundle>>,
    pub depth: usize,
}

/// The child versions this definition pins: its child steps' and its workflow tools' (07-D2).
fn pinned_versions(
    parsed: &compiler::ParsedWorkflow,
    tools: &[ToolSnapshot],
) -> BTreeSet<VersionId> {
    parsed
        .controls
        .values()
        .map(|c| c.child_version_id)
        .chain(tools.iter().filter_map(|tool| match tool.kind() {
            ToolKind::Workflow(pin) => Some(pin.version_id),
            _ => None,
        }))
        .collect()
}

pub(super) fn capture(
    company_id: CompanyId,
    parsed: &compiler::ParsedWorkflow,
    children: Vec<Arc<PublishedBundle>>,
    tools: &[ToolSnapshot],
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
            parameter_schema: definition
                .parameter_schema
                .clone()
                .expect("compiled schema"),
            resources: definition.resources.clone(),
        });
        result.children.insert(definition.version_id, child);
    }
    let expected = pinned_versions(parsed, tools);
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
    for tool in tools {
        if let ToolKind::Workflow(pin) = tool.kind() {
            workflow_tool(parsed, tool, pin, &result.children[&pin.version_id])?;
        }
    }
    result.graph = dependency_graph(parsed, &result.children, tools)?;
    Ok(result)
}

/// A workflow tool's frozen contract is exactly its pinned child's (07-D2 §D2.2): the child's
/// input and output schemas, parameters the child accepts, and only resource slots the parent
/// also declares with the same kind and contract, which the child receives by slot name.
fn workflow_tool(
    parsed: &compiler::ParsedWorkflow,
    tool: &ToolSnapshot,
    pin: &WorkflowToolPin,
    child: &PublishedBundle,
) -> Result<(), Diagnostic> {
    let fail = |reason: &str| {
        Diagnostic::at(
            "publication.workflow_tool",
            &format!("Workflow tool {}: {reason}", tool.contract.name.as_str()),
            "/steps",
            compiler::input_span(&parsed.locations, "/steps"),
        )
    };
    let definition = child.compiled.graph().definition();
    if definition.workflow_id != pin.workflow_id
        || tool.contract.connection.is_some()
        || definition.input_schema.as_ref() != Some(&tool.contract.input_schema)
        || definition.output_schema.as_ref() != Some(&tool.contract.output_schema)
    {
        return Err(fail("its contract is not the pinned workflow's"));
    }
    child
        .compiled
        .validate_params(&pin.params)
        .map_err(|_| fail("its parameters do not satisfy the pinned workflow"))?;
    if definition
        .resources
        .iter()
        .any(|slot| !parsed.definition.resources.contains(slot))
    {
        return Err(fail(
            "declare each of the pinned workflow's resource slots with the same kind and contract",
        ));
    }
    Ok(())
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
    tools: &[ToolSnapshot],
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
    // Workflow tools are edges like child steps, so recursion through a tool is rejected too.
    graph.insert(
        parsed.definition.workflow_id,
        parsed
            .controls
            .values()
            .map(|c| c.child_workflow_id)
            .chain(tools.iter().filter_map(|tool| match tool.kind() {
                ToolKind::Workflow(pin) => Some(pin.workflow_id),
                _ => None,
            }))
            .collect(),
    );
    Ok(graph)
}
