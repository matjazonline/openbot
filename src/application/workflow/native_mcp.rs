//! The private native broker compiles the same one-step MCP graph as an authored workflow.
//! The seal proves compilation and bounded input, not current actor or resource authority.
use super::binding::{BindingConfiguration, ConfiguredBinding};
use super::publication::{self, DependencySnapshots, ToolSnapshot};
use super::{WorkflowActor, canonical, compiler};
use crate::app_error::{AppError, AppResult};
use crate::application::agent::ResolvedAgentTool;
use crate::application::local_agents::mcp::MAX_ARGUMENT_BYTES;
use crate::domain::workflow::{
    BindingRevision, CompanyId, ResourceName, RuntimeResourceId, VersionId, WorkflowBindingId,
    WorkflowId,
};
use crate::entities::agent_capability::AgentBindingSource;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

pub struct NativeBrokerIdentity {
    pub company: CompanyId,
    pub actor: WorkflowActor,
    pub workflow: WorkflowId,
    pub version: VersionId,
    pub binding: WorkflowBindingId,
}

/// No deserializer or public field permits substituting a graph after this check.
pub struct PreparedNativeMcp {
    binding: ConfiguredBinding,
    input: Value,
    argument_digest: String,
}

impl PreparedNativeMcp {
    pub fn compile(
        decoder: &dyn compiler::SourceDecoder,
        identity: NativeBrokerIdentity,
        tool: &ResolvedAgentTool,
        arguments: &Value,
        deadline: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<Self> {
        let AgentBindingSource::Mcp {
            connection,
            operation,
            ..
        } = &tool.binding.source
        else {
            return Err(AppError::BadRequest(
                "Native tools require a company MCP binding".into(),
            ));
        };
        let arguments = canonical_arguments(arguments)?;
        let argument_digest = canonical::digest(&arguments)?;
        let source = source(&identity, tool, operation.as_str());
        let bundle = publication::freeze(
            decoder.decode(&source).map_err(diagnostic)?,
            identity.company,
            identity.version,
            DependencySnapshots {
                agents: vec![],
                tools: vec![ToolSnapshot {
                    company_id: identity.company,
                    contract: super::registry::ToolContract {
                        name: operation.clone(),
                        connection: Some(slot()?),
                        input_schema: tool.input_schema.clone(),
                        output_schema: tool.output_schema.clone(),
                    },
                    policy: tool.policy.clone(),
                    workflow: None,
                }],
            },
            vec![],
        )
        .map_err(diagnostic)?;
        let binding = ConfiguredBinding::new(
            BindingConfiguration {
                id: identity.binding,
                revision: BindingRevision::new(1).expect("positive initial revision"),
                company_id: identity.company,
                params: json!({}),
                resources: BTreeMap::from([(slot()?, RuntimeResourceId::new(*connection))]),
                run_as: identity.actor,
                message_trigger: None,
            },
            Arc::new(bundle),
        )
        .map_err(diagnostic)?;
        let input = json!({"arguments":arguments,"deadline":deadline.to_rfc3339()});
        binding
            .bundle()
            .compiled()
            .validate_input(&input)
            .map_err(diagnostic)?;
        Ok(Self {
            binding,
            input,
            argument_digest,
        })
    }

    pub fn binding(&self) -> &ConfiguredBinding {
        &self.binding
    }
    pub fn input(&self) -> &Value {
        &self.input
    }
    pub fn argument_digest(&self) -> &str {
        &self.argument_digest
    }
}

pub fn canonical_arguments(arguments: &Value) -> AppResult<Value> {
    if !arguments.is_object() {
        return Err(AppError::BadRequest(
            "MCP arguments must be an object".into(),
        ));
    }
    let bounded = canonical::canonical(arguments, MAX_ARGUMENT_BYTES)?;
    if contains_nul(&bounded) {
        return Err(AppError::BadRequest(
            "MCP arguments contain a NUL character".into(),
        ));
    }
    Ok(bounded)
}

fn contains_nul(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains('\0'),
        Value::Array(items) => items.iter().any(contains_nul),
        Value::Object(items) => items
            .iter()
            .any(|(key, value)| key.contains('\0') || contains_nul(value)),
        _ => false,
    }
}

pub fn argument_digest(arguments: &Value) -> AppResult<String> {
    canonical::digest(&canonical_arguments(arguments)?)
}

fn slot() -> AppResult<ResourceName> {
    ResourceName::parse("service").map_err(diagnostic)
}

fn diagnostic(error: impl std::fmt::Debug) -> AppError {
    AppError::BadRequest(format!("Invalid native MCP workflow: {error:?}"))
}

fn source(identity: &NativeBrokerIdentity, tool: &ResolvedAgentTool, operation: &str) -> String {
    let mut inputs = json!({"connection":{"literal":"service"},"tool":{"literal":operation},
        "arguments":{"ref":"/input/arguments"}});
    if tool.approval_required {
        inputs["approval"] = json!({"object":{
            "reviewers":{"literal":[{"kind":"user","id":identity.actor.user_id()}]},
            "deadline":{"ref":"/input/deadline"}
        }});
    }
    json!({"format_version":1,"workflow_id":identity.workflow,
        "input_schema":{"type":"object","additionalProperties":false,"required":["arguments","deadline"],
            "properties":{"arguments":tool.input_schema,"deadline":{"type":"string","maxLength":64}}},
        "parameter_schema":{"type":"object","additionalProperties":false},"output_schema":true,
        "resources":[{"slot":"service","kind":"mcp"}],"entry":"start",
        "steps":{"start":{"type":"mcp.call","with":inputs,"routes":{"success":"$end"}}},
        "limits":{"max_steps":1,"max_context_bytes":1_048_576}}).to_string()
}

#[cfg(test)]
mod tests;
