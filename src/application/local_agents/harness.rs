//! Portable projection of one immutable published agent. No credentials or machine paths.
use crate::app_error::{AppError, AppResult};
use crate::application::agent::{ResolvedAgentDefinition, ResolvedAgentTool};
use crate::domain::local_agents::{LocalPermissions, NativeBundleDigest};
use crate::domain::workflow::TypeName;
use crate::entities::skill_bundle::{SkillFile, native_skill_markdown};
use base64::Engine;
use serde::{Deserialize, Serialize};

pub const MAX_NATIVE_BUNDLE_BYTES: usize = 2 * 1024 * 1024;

pub struct NativeBundleRequest {
    pub verifier: super::CredentialVerifier,
    pub identity: super::protocol::ConnectionIdentity,
    pub operation: crate::domain::local_agents::OperationId,
}
pub struct NativeBundleResponse {
    pub bytes: Vec<u8>,
    pub digest: NativeBundleDigest,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeBundleFormat {
    OpenbotNativeV2,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeSkill {
    pub name: TypeName,
    pub entry: String,
    pub files: Vec<SkillFile>,
}
/// Encode the complete canonical contract without changing JSON schema numbers
/// through JavaScript's number representation or duplicating its escaped text.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeMcpTool {
    pub binding_id: crate::entities::agent_capability::AgentBindingId,
    pub alias: crate::entities::agent_capability::AgentToolAlias,
    pub contract_base64: String,
}
impl NativeMcpTool {
    fn from_tool(tool: &ResolvedAgentTool) -> AppResult<Self> {
        let bytes = serde_json::to_vec(tool)
            .map_err(|_| AppError::Internal("Cannot encode native MCP contract".into()))?;
        Ok(Self {
            binding_id: tool.binding.id,
            alias: tool.binding.alias.clone(),
            contract_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        })
    }

    pub fn contract(&self) -> AppResult<ResolvedAgentTool> {
        let invalid = || AppError::Internal("Invalid frozen native MCP contract".into());
        if self.contract_base64.len() > 4 * 65_536_usize.div_ceil(3) {
            return Err(invalid());
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&self.contract_base64)
            .map_err(|_| invalid())?;
        let tool: ResolvedAgentTool = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if bytes.len() > crate::entities::agent_capability::MAX_AGENT_BINDING_BYTES
            || tool.binding.id != self.binding_id
            || tool.binding.alias != self.alias
            || !matches!(
                tool.binding.source,
                crate::entities::agent_capability::AgentBindingSource::Mcp { .. }
            )
        {
            return Err(invalid());
        }
        Ok(tool)
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeBundle {
    pub format: NativeBundleFormat,
    pub instructions: String,
    pub permissions: LocalPermissions,
    pub skills: Vec<NativeSkill>,
    pub tools: Vec<NativeMcpTool>,
}
impl NativeBundle {
    pub fn from_definition(definition: &ResolvedAgentDefinition) -> AppResult<Self> {
        definition.validate()?;
        if definition.tools.iter().any(|tool| {
            !matches!(
                tool.binding.source,
                crate::entities::agent_capability::AgentBindingSource::Mcp { .. }
            )
        }) {
            return Err(AppError::BadRequest(
                "Native agent tools must use existing company MCP connections".into(),
            ));
        }
        let skills = definition
            .skills
            .iter()
            .map(|assigned| {
                let skill = &assigned.skill;
                Ok(NativeSkill {
                    name: skill.name.clone(),
                    entry: native_skill_markdown(
                        skill.name.as_str(),
                        &skill.description,
                        &skill.instructions,
                    )
                    .map_err(AppError::BadRequest)?,
                    files: skill.files.clone(),
                })
            })
            .collect::<AppResult<_>>()?;
        Ok(Self {
            format: NativeBundleFormat::OpenbotNativeV2,
            instructions: definition.instructions.clone(),
            permissions: definition.native_permissions,
            skills,
            tools: definition
                .tools
                .iter()
                .map(NativeMcpTool::from_tool)
                .collect::<AppResult<_>>()?,
        })
    }
    pub fn canonical_content(&self) -> AppResult<Vec<u8>> {
        let bytes = serde_json::to_vec(self)
            .map_err(|_| AppError::Internal("Cannot encode native bundle".into()))?;
        if bytes.len() > MAX_NATIVE_BUNDLE_BYTES {
            return Err(AppError::BadRequest("Native bundle exceeds 2 MiB".into()));
        }
        Ok(bytes)
    }
    pub fn digest(&self) -> AppResult<NativeBundleDigest> {
        Ok(NativeBundleDigest::of(&self.canonical_content()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::agent::{
        AgentKey, MAX_AGENT_DEFINITION_BYTES, ResolvedAgentSkill, SkillSnapshot,
    };
    use crate::domain::workflow::CompanyId;
    use crate::entities::agent_capability::{AgentId, AgentSkillAssignmentId};
    use uuid::Uuid;

    fn mcp_tool(index: usize) -> ResolvedAgentTool {
        use crate::application::agent::{ActionEffect, ActionRecovery, ApprovedActionPolicy};
        use crate::entities::agent_capability::{
            AgentBindingId, AgentBindingSource, AgentToolAlias, AgentToolBinding,
        };
        ResolvedAgentTool {
            binding: AgentToolBinding {
                id: AgentBindingId::new(Uuid::new_v4()),
                alias: AgentToolAlias::parse(format!("mcp_{index}")).unwrap(),
                source: AgentBindingSource::Mcp {
                    connection: Uuid::new_v4(),
                    operation: TypeName::parse(format!("lookup_{index}")).unwrap(),
                    reviewed_version: Uuid::new_v4(),
                    remote_name: format!("lookup_{index}").try_into().unwrap(),
                },
            },
            input_schema: serde_json::json!({"type":"object","description":""}),
            output_schema: serde_json::json!(true),
            policy: ApprovedActionPolicy {
                capability: TypeName::parse("mcp.call").unwrap(),
                policy_revision: 1,
                effect: ActionEffect::Write,
                recovery: ActionRecovery::Reconcile,
            },
            approval_required: true,
        }
    }

    fn skill_definition() -> ResolvedAgentDefinition {
        let company = CompanyId::new(Uuid::new_v4());
        let mut definition = ResolvedAgentDefinition {
            company_id: company,
            agent_id: AgentId::new(Uuid::new_v4()),
            key: AgentKey::parse("native-budget").unwrap(),
            instructions: "\"".repeat(16 * 1024),
            native_permissions: LocalPermissions::Full,
            model: None,
            tools: vec![],
            skills: vec![],
        };
        for i in 0..16 {
            definition.skills.push(ResolvedAgentSkill {
                assignment_id: AgentSkillAssignmentId::new(Uuid::new_v4()),
                skill: SkillSnapshot {
                    company_id: company,
                    skill_id: Uuid::new_v4(),
                    name: TypeName::parse(format!("skill-{i}")).unwrap(),
                    description: "🐚".repeat(crate::entities::skill::MAX_SKILL_DESCRIPTION_CHARS),
                    instructions: "x".into(),
                    required_tools: vec![],
                    files: if i == 0 {
                        (0..6)
                            .map(|j| {
                                SkillFile::from_bytes(
                                    format!("assets/{j}.bin"),
                                    &vec![0; 64 * 1024],
                                    false,
                                )
                                .unwrap()
                            })
                            .collect()
                    } else {
                        vec![]
                    },
                },
            });
        }
        definition.tools = (0..32).map(mcp_tool).collect();
        definition
    }

    fn assert_headroom(definition: &ResolvedAgentDefinition) {
        let source = definition.canonical_content().unwrap();
        assert!(source.len() > MAX_AGENT_DEFINITION_BYTES * 99 / 100);
        let bundle = NativeBundle::from_definition(definition).unwrap();
        let bytes = bundle.canonical_content().unwrap();
        assert!(
            bytes.len() < MAX_NATIVE_BUNDLE_BYTES * 3 / 4,
            "{} bytes leaves insufficient native bundle headroom",
            bytes.len()
        );
        assert_eq!(bundle.digest().unwrap(), NativeBundleDigest::of(&bytes));
        for (expected, actual) in definition.tools.iter().zip(&bundle.tools) {
            assert_eq!(
                serde_json::to_value(expected).unwrap(),
                serde_json::to_value(actual.contract().unwrap()).unwrap()
            );
        }
    }

    #[test]
    fn native_bundle_maximum_projection_retains_twenty_five_percent_headroom() {
        let mut definition = skill_definition();
        // Fill the actual canonical definition budget with JSON-escaped instruction text.
        for i in 0..16 {
            let len = serde_json::to_vec(&definition).unwrap().len();
            let available = (MAX_AGENT_DEFINITION_BYTES - len) / 2;
            let count = available.min(16 * 1024 - 1);
            definition.skills[i]
                .skill
                .instructions
                .push_str(&"\"".repeat(count));
        }
        assert_headroom(&definition);
    }

    #[test]
    fn native_bundle_maximum_mcp_contract_projection_retains_twenty_five_percent_headroom() {
        let mut definition = skill_definition();
        definition.skills.clear();
        for i in 0..definition.tools.len() {
            let available =
                MAX_AGENT_DEFINITION_BYTES - serde_json::to_vec(&definition).unwrap().len();
            definition.tools[i].input_schema["description"] =
                serde_json::json!("\"".repeat((available.saturating_sub(32) / 2).min(30 * 1024)));
        }
        assert_headroom(&definition);
    }

    #[test]
    fn native_bundle_mcp_contract_preserves_large_schema_numbers_and_refuses_other_tools() {
        use crate::entities::agent_capability::AgentBindingSource;
        let mut definition = skill_definition();
        definition.tools.truncate(1);
        definition.tools[0].input_schema["maximum"] = serde_json::json!(u64::MAX);
        let bundle = NativeBundle::from_definition(&definition).unwrap();
        assert_eq!(
            bundle.tools[0].contract().unwrap().input_schema["maximum"],
            serde_json::json!(u64::MAX)
        );
        definition.tools[0].binding.source = AgentBindingSource::Http {
            connection: Uuid::new_v4(),
            operation: TypeName::parse("request").unwrap(),
        };
        assert!(NativeBundle::from_definition(&definition).is_err());
    }
}
