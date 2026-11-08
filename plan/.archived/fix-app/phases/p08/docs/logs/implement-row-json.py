from pathlib import Path
# Optional presentation contracts decode beside the row, with the canonical identifier.
for name in ('src/adapters/persistence/thread/message.rs','src/adapters/persistence/thread/views.rs'):
 p=Path(name);s=p.read_text();old='''    response_contract:
        Option<sqlx::types::Json<crate::entities::response_contract::ResponseContract>>,'''
 if name.endswith('message.rs'):old='    pub '+old.strip().replace('response_contract:', 'response_contract:',1)
 # Match each exact DB field independent of its public visibility.
 import re
 s,n=re.subn(r'(    (?:pub )?response_contract:)\n        Option<sqlx::types::Json<crate::entities::response_contract::ResponseContract>>,',r'\1 Option<Value>,',s)
 assert n==1,(name,n)
 if name.endswith('message.rs'):
  old='response_contract: db.response_contract.map(|contract| contract.0),';new='response_contract: decode_response_contract(CanonicalMessageId::new(db.canonical_id), db.response_contract),'
  assert old in s;s=s.replace(old,new)
  i=s.index('pub(super) fn decode_attachments(')
  # Keep the attachment function's existing documentation with that function.
  i=s.rfind('\n///',0,i)
  s=s[:i]+'''
/// A malformed optional form cannot make neighboring messages unreadable.
pub(super) fn decode_response_contract(
    message_id: CanonicalMessageId,
    value: Option<Value>,
) -> Option<crate::entities::response_contract::ResponseContract> {
    match serde_json::from_value(value?) {
        Ok(contract) => Some(contract),
        Err(error) => {
            tracing::warn!(message_id = %message_id.as_uuid(), error = %error,
                "Stored message response contract is invalid");
            None
        }
    }
}
'''+s[i:]
 else:
  old='use super::message::decode_attachments;';assert old in s;s=s.replace(old,'use super::message::{decode_attachments, decode_response_contract};')
  old='response_contract: self.response_contract.map(|value| value.0),';assert old in s;s=s.replace(old,'response_contract: decode_response_contract(CanonicalMessageId::new(self.canonical_id), self.response_contract),')
 p.write_text(s)
# Delivery corruption stays a hard failure, with the intended delivery's identity.
p=Path('src/adapters/persistence/delivery/enqueue.rs');s=p.read_text();old='''    let structured: Option<
        sqlx::types::Json<crate::services::response_contract::StructuredResponse>,
    > = sqlx::query_scalar(''';new='''    let structured: Option<serde_json::Value> = sqlx::query_scalar(''';assert old in s;s=s.replace(old,new)
old='''    if let Some(response) = structured {
        crate::adapters::response_schema::validate_delivery(&response.0, delivery)?;
    }''';new='''    if let Some(value) = structured {
        let response = serde_json::from_value::<crate::services::response_contract::StructuredResponse>(value)
            .map_err(|error| AppError::Internal(format!(
                "Invalid structured response for delivery {}: {error}", delivery.id.as_uuid()
            )))?;
        crate::adapters::response_schema::validate_delivery(&response, delivery)?;
    }''';assert old in s;s=s.replace(old,new);p.write_text(s)
# Preserve exact decoder/validation errors, using the actual persisted key.
p=Path('src/adapters/persistence/harness_checkpoint.rs');s=p.read_text();old='''pub(super) fn decode(value: serde_json::Value) -> AppResult<RunCheckpoint> {
    let run: RunCheckpoint = serde_json::from_value(value)
        .map_err(|_| AppError::BadRequest("Malformed harness checkpoint".into()))?;
    run.validate()?;''';new='''pub(super) fn decode(run_id: uuid::Uuid, value: serde_json::Value) -> AppResult<RunCheckpoint> {
    let run: RunCheckpoint = serde_json::from_value(value)
        .map_err(|error| invalid_checkpoint(run_id, error))?;
    run.validate().map_err(|error| invalid_checkpoint(run_id, error))?;''';assert old in s;s=s.replace(old,new)
s=s.replace('''        )?;
    }
    Ok(run)
}''','''        ).map_err(|error| invalid_checkpoint(run_id, error))?;
    }
    Ok(run)
}

fn invalid_checkpoint(run_id: uuid::Uuid, error: impl std::fmt::Display) -> AppError {
    AppError::Internal(format!("Invalid stored harness checkpoint {run_id}: {error}"))
}''',1);p.write_text(s)
p=Path('src/adapters/persistence/workflow/checkpoints.rs');s=p.read_text();assert 'struct Saved {\n    checkpoint:' in s;s=s.replace('struct Saved {\n    checkpoint:', 'struct Saved {\n    id: Uuid,\n    checkpoint:');old='SELECT checkpoint,workflow_prepared,model_blocked_decision';assert s.count(old)==1;s=s.replace(old,'SELECT id,checkpoint,workflow_prepared,model_blocked_decision');assert s.count('decode(saved.checkpoint)')==3;s=s.replace('decode(saved.checkpoint)','decode(saved.id, saved.checkpoint)');assert s.count('decode(run.checkpoint)')==1;s=s.replace('decode(run.checkpoint)','decode(run.id, run.checkpoint)');p.write_text(s)
# One catalog validator remains authoritative for strict writes and defensive reads.
p=Path('src/application/use_cases/mcp.rs');s=p.read_text();a=s.index('    pub fn validate(&self) -> AppResult<()> {');b=s.index('\n    }',a)+6;part=s[a:b]
slug='        super::channel::validate_slug(&self.slug, super::channel::SlugKind::AgentSlug)?;\n';assert slug in part
body=part[part.index(slug)+len(slug):];body=body[:body.rfind('\n    }')]
body=body.replace('&self.discovered_tools','discovered_tools').replace('&self.tool_grants','tool_grants').replace('self.discovered_tools','discovered_tools').replace('self.tool_grants','tool_grants')
body='\n'.join(line[4:] if line.startswith('    ') else line for line in body.splitlines())
replacement='''    pub fn validate(&self) -> AppResult<()> {
        super::channel::validate_slug(&self.slug, super::channel::SlugKind::AgentSlug)?;
        validate_catalog(&self.discovered_tools, &self.tool_grants)
    }'''
s=s[:a]+replacement+s[b:];i=s.index('\nfn invalid(')
s=s[:i]+'''
/// Validate the reviewed discovery and grants together; callers cannot widen a bad catalog.
pub(crate) fn validate_catalog(
    discovered_tools: &[McpDiscoveredTool],
    tool_grants: &[McpToolName],
) -> AppResult<()> {
'''+body+'\n}\n'+s[i:];p.write_text(s)
p=Path('src/adapters/persistence/mcp.rs');s=p.read_text();a=s.index('        let write = McpConnectionWrite {');b=s.index('        Ok(Self {',a)
s=s[:a]+'''        let mut write = McpConnectionWrite {
            slug: row.slug.into(),
            endpoint: row.endpoint_url.try_into().map_err(AppError::BadRequest)?,
            enabled: row.enabled,
            auth: auth.clone(),
            discovered_tools: Vec::new(),
            tool_grants: Vec::new(),
        };
        // Validate ordinary settings strictly before the optional catalog is considered.
        write.validate()?;
        match read_catalog(row.discovery_json, row.tool_grants) {
            Ok(catalog) => {
                write.discovered_tools = catalog.tools;
                write.tool_grants = catalog.grants;
            }
            Err(error) => tracing::warn!(connection_id = %row.id, error,
                "Stored MCP discovery was quarantined"),
        }
'''+s[b:]
i=s.index('\nfn invalid(')
s=s[:i]+'''
struct ReadCatalog {
    tools: Vec<McpDiscoveredTool>,
    grants: Vec<McpToolName>,
}

fn read_catalog(value: serde_json::Value, grants: Vec<String>) -> Result<ReadCatalog, &'static str> {
    let tools = serde_json::from_value(value).map_err(|_| "Malformed stored MCP discovery")?;
    let grants = grants.into_iter().map(McpToolName::try_from).collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Malformed stored MCP grants")?;
    validate_catalog(&tools, &grants).map_err(|_| "Invalid stored MCP discovery catalog")?;
    Ok(ReadCatalog { tools, grants })
}
'''+s[i:];p.write_text(s)
print('Row-aware JSON decoding and fail-closed MCP catalog quarantine applied')
