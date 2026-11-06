use super::*;

const API_PREVIEW: &str = "/api/companies/{company_id}/agents/{agent_id}/preview";
const API_PUBLISH: &str = "/api/companies/{company_id}/agents/{agent_id}/publish";
const API_REVOKE: &str = "/api/companies/{company_id}/agents/{agent_id}/revoke";
const API_DEACTIVATE: &str = "/api/companies/{company_id}/agents/{agent_id}/deactivate";
const UI_PREVIEW: &str = "/ui/agents/{agent_id}/preview";
const UI_PUBLISH: &str = "/ui/agents/{agent_id}/publish";
const UI_REVOKE: &str = "/ui/agents/{agent_id}/revoke";
const UI_DEACTIVATE: &str = "/ui/agents/{agent_id}/deactivate";

#[cfg(test)]
pub(super) const WRITE_ROUTES: [&str; 8] = [
    API_PREVIEW,
    API_PUBLISH,
    API_REVOKE,
    API_DEACTIVATE,
    UI_PREVIEW,
    UI_PUBLISH,
    UI_REVOKE,
    UI_DEACTIVATE,
];

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(API_PREVIEW, post(preview_json))
        .route(API_PUBLISH, post(publish_json))
        .route(API_REVOKE, post(revoke_json))
        .route(API_DEACTIVATE, post(deactivate_json))
        .route(UI_PREVIEW, post(preview_form))
        .route(UI_PUBLISH, post(publish_form))
        .route(UI_REVOKE, post(revoke_form))
        .route(UI_DEACTIVATE, post(deactivate_form))
}

/// Deliberately excludes immutable instructions, skill text, schemas and workflow parameters.
#[derive(Serialize)]
struct ValidationSummary {
    draft_revision: AgentDraftRevision,
    active_revision: Option<AgentRevisionId>,
    tool_count: usize,
    skill_count: usize,
}
impl From<AgentPreview> for ValidationSummary {
    fn from(preview: AgentPreview) -> Self {
        Self {
            draft_revision: preview.draft_revision,
            active_revision: preview.active_revision,
            tool_count: preview.definition.tools.len(),
            skill_count: preview.definition.skills.len(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PublicationInput {
    expected_draft_revision: AgentDraftRevision,
    expected_active_revision: Option<AgentRevisionId>,
    key: AgentPublicationKey,
}
impl PublicationInput {
    fn command(&self, scope: AgentPublicationScope) -> PublishAgent {
        PublishAgent {
            scope,
            expected_draft_revision: self.expected_draft_revision,
            expected_active_revision: self.expected_active_revision,
            key: self.key.clone(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PublicationForm {
    pub(super) expected_draft_revision: AgentDraftRevision,
    #[serde(default)]
    pub(super) expected_active_revision: String,
    pub(super) key: AgentPublicationKey,
}
impl PublicationForm {
    fn input(self) -> AppResult<PublicationInput> {
        let active = if self.expected_active_revision.is_empty() {
            None
        } else {
            Some(AgentRevisionId::new(
                Uuid::parse_str(&self.expected_active_revision)
                    .map_err(|_| AppError::BadRequest("Invalid active agent revision".into()))?,
            ))
        };
        Ok(PublicationInput {
            expected_draft_revision: self.expected_draft_revision,
            expected_active_revision: active,
            key: self.key,
        })
    }
}

#[derive(Serialize)]
struct PublicationReceipt {
    id: AgentRevisionId,
    draft_revision: AgentDraftRevision,
    digest: String,
}
impl From<PublishedAgentRevision> for PublicationReceipt {
    fn from(revision: PublishedAgentRevision) -> Self {
        Self {
            id: revision.id,
            draft_revision: revision.draft_revision,
            digest: revision
                .digest
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        }
    }
}

async fn preview_json(
    workspace: Workspace,
    Path((company, agent)): Path<(Uuid, Uuid)>,
) -> AppResult<impl IntoResponse> {
    let scope = scope(workspace.user_id, company, agent)?;
    let preview = workspace
        .workflow
        .agent_publications()
        .preview_agent(scope)
        .await?;
    fresh_json(&workspace, scope, ValidationSummary::from(preview)).await
}

async fn publish_json(
    workspace: Workspace,
    Path((company, agent)): Path<(Uuid, Uuid)>,
    Json(input): Json<PublicationInput>,
) -> AppResult<impl IntoResponse> {
    let scope = scope(workspace.user_id, company, agent)?;
    let published = workspace
        .workflow
        .agent_publications()
        .publish_agent(input.command(scope))
        .await?;
    fresh_json(&workspace, scope, PublicationReceipt::from(published)).await
}

async fn preview_form(
    workspace: Workspace,
    Path(agent): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
) -> AppResult<Html<String>> {
    let scope = scope(workspace.user_id, query.company_id, agent)?;
    let message = match workspace
        .workflow
        .agent_publications()
        .preview_agent(scope)
        .await
    {
        Ok(preview) => format!(
            "Saved draft {} is valid: {} tools and {} skills. It is ready to publish.",
            preview.draft_revision.get(),
            preview.definition.tools.len(),
            preview.definition.skills.len()
        ),
        Err(error) => editable_failure(error)?,
    };
    panel_response(&workspace, scope, None, Some(&message)).await
}

pub(super) async fn publish_form(
    workspace: Workspace,
    Path(agent): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
    Form(form): Form<PublicationForm>,
) -> AppResult<Html<String>> {
    let scope = scope(workspace.user_id, query.company_id, agent)?;
    let input = form.input()?;
    let command = input.command(scope);
    let (retry, message) = match workspace
        .workflow
        .agent_publications()
        .publish_agent(command.clone())
        .await
    {
        Ok(revision) => (
            None,
            format!(
                "Publication confirmed for revision {}. The current active revision is shown below.",
                revision.id.as_uuid()
            ),
        ),
        Err(error) => (Some(&command), editable_failure(error)?),
    };
    panel_response(&workspace, scope, retry, Some(&message)).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RevokeInput {
    binding: Option<AgentBindingId>,
    skill: Option<AgentSkillAssignmentId>,
}
impl RevokeInput {
    fn identity(self) -> AppResult<AgentGrantIdentity> {
        match (self.binding, self.skill) {
            (Some(binding), None) => Ok(AgentGrantIdentity::Binding(binding)),
            (None, Some(skill)) => Ok(AgentGrantIdentity::Skill(skill)),
            _ => Err(AppError::BadRequest(
                "Select exactly one binding or skill assignment to revoke".into(),
            )),
        }
    }
}

async fn revoke_json(
    workspace: Workspace,
    Path((company, agent)): Path<(Uuid, Uuid)>,
    Json(input): Json<RevokeInput>,
) -> AppResult<impl IntoResponse> {
    let scope = scope(workspace.user_id, company, agent)?;
    workspace
        .workflow
        .agent_publications()
        .revoke_agent_grant(scope, input.identity()?)
        .await?;
    fresh_json(&workspace, scope, serde_json::json!({"revoked":true})).await
}

async fn revoke_form(
    workspace: Workspace,
    Path(agent): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
    Form(input): Form<RevokeInput>,
) -> AppResult<Html<String>> {
    let scope = scope(workspace.user_id, query.company_id, agent)?;
    let message = match workspace
        .workflow
        .agent_publications()
        .revoke_agent_grant(scope, input.identity()?)
        .await
    {
        Ok(()) => "Access revoked. Running work that needs this capability may stop.".to_owned(),
        Err(error) => editable_failure(error)?,
    };
    panel_response(&workspace, scope, None, Some(&message)).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeactivateInput {
    expected_active_revision: AgentRevisionId,
}

async fn deactivate_json(
    workspace: Workspace,
    Path((company, agent)): Path<(Uuid, Uuid)>,
    Json(input): Json<DeactivateInput>,
) -> AppResult<impl IntoResponse> {
    let scope = scope(workspace.user_id, company, agent)?;
    workspace
        .workflow
        .agent_publications()
        .deactivate_agent(scope, Some(input.expected_active_revision))
        .await?;
    fresh_json(&workspace, scope, serde_json::json!({"deactivated":true})).await
}

async fn deactivate_form(
    workspace: Workspace,
    Path(agent): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
    Form(input): Form<DeactivateInput>,
) -> AppResult<Html<String>> {
    let scope = scope(workspace.user_id, query.company_id, agent)?;
    let message = match workspace
        .workflow
        .agent_publications()
        .deactivate_agent(scope, Some(input.expected_active_revision))
        .await
    {
        Ok(()) => {
            "Agent deactivated for new runs. Existing runs keep their selected revision.".to_owned()
        }
        Err(error) => editable_failure(error)?,
    };
    panel_response(&workspace, scope, None, Some(&message)).await
}

#[cfg(test)]
#[path = "publication_tests.rs"]
mod tests;
