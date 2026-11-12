use crate::use_cases::{company::CompanyUseCases, thread::ThreadUseCases, user::UserUseCases};
use crate::{
    adapters::http::{auth::AuthError, pages::local_agents as pages},
    app_error::{AppError, AppResult},
    application::local_agents::{management::*, *},
    domain::{
        entities::{thread::Thread, user::Viewer},
        local_agents::*,
    },
};
use axum::{
    Form, Router,
    extract::{DefaultBodyLimit, FromRef, FromRequestParts, Path, Query},
    http::request::Parts,
    middleware,
    response::{Html, Redirect},
    routing::{get, post},
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

pub fn router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    Arc<CompanyUseCases>: FromRef<S>,
    Arc<ThreadUseCases>: FromRef<S>,
    Arc<UserUseCases>: FromRef<S>,
    Arc<LocalMachineUseCases>: FromRef<S>,
    Arc<LocalAgentManagement>: FromRef<S>,
{
    Router::new()
        .route("/ui/companies/{company}/local-agents", get(page))
        .route("/ui/companies/{company}/local-agents/pairings", post(pair))
        .route(
            "/ui/companies/{company}/local-agents/pairings/{pair}/cancel",
            post(cancel_pair),
        )
        .route(
            "/ui/companies/{company}/local-agents/machines/{machine}/state",
            post(change),
        )
        .route(
            "/ui/companies/{company}/local-agents/machines/{machine}/projects",
            post(projects),
        )
        .route(
            "/ui/companies/{company}/local-agents/machines/{machine}/test",
            post(test),
        )
        .route(
            "/ui/companies/{company}/local-agents/profiles",
            post(create_profile),
        )
        .route(
            "/ui/companies/{company}/local-agents/profiles/{profile}",
            post(update_profile),
        )
        .route(
            "/ui/companies/{company}/local-agents/profiles/{profile}/delete",
            post(delete_profile),
        )
        .route("/ui/companies/{company}/local-agents/sessions", post(start))
        .route(
            "/ui/companies/{company}/local-agents/sessions/{session}/commands",
            post(control),
        )
        .layer(DefaultBodyLimit::max(MAX_PROMPT_BYTES * 6 + 4096))
        .layer(middleware::map_response(
            super::local_machines::private_response,
        ))
}

struct Workspace {
    companies: Arc<CompanyUseCases>,
    threads: Arc<ThreadUseCases>,
    machines: Arc<LocalMachineUseCases>,
    agents: Arc<LocalAgentManagement>,
    viewer: Viewer,
}
impl<S> FromRequestParts<S> for Workspace
where
    S: Send + Sync,
    Arc<CompanyUseCases>: FromRef<S>,
    Arc<ThreadUseCases>: FromRef<S>,
    Arc<UserUseCases>: FromRef<S>,
    Arc<LocalMachineUseCases>: FromRef<S>,
    Arc<LocalAgentManagement>: FromRef<S>,
{
    type Rejection = AuthError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, AuthError> {
        Ok(Self {
            viewer: Viewer::from_request_parts(parts, state).await?,
            companies: FromRef::from_ref(state),
            threads: FromRef::from_ref(state),
            machines: FromRef::from_ref(state),
            agents: FromRef::from_ref(state),
        })
    }
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    machine: Option<MachineId>,
    profile: Option<ProfileId>,
    session: Option<SessionId>,
    provider: Option<LocalProvider>,
    model: Option<ModelId>,
    channel: Option<Uuid>,
    conversation: Option<Uuid>,
    test: Option<OperationId>,
    after: Option<MachineId>,
    after_profile: Option<ProfileId>,
    after_pairing: Option<PairingId>,
}
impl Workspace {
    async fn render(
        &self,
        company: Uuid,
        selection: Selection,
        pairing: Option<CreatedPairing>,
    ) -> AppResult<Html<String>> {
        let actor = self.viewer.user_id;
        // Management authority precedes reads of machine/profile inventory or selected data.
        let profiles = self
            .agents
            .profiles(actor, company, selection.after_profile)
            .await?;
        let native_agents = self.agents.native_agent_choices(actor, company).await?;
        let access = self.companies.readable_company(actor, company).await?;
        let machines = self
            .machines
            .machines(actor, company, selection.after)
            .await?;
        let pairings = self
            .machines
            .list_pairings(actor, company, selection.after_pairing)
            .await?;
        let profile = match selection.profile {
            Some(id) => Some(
                self.agents
                    .profile(actor, company, id)
                    .await?
                    .ok_or_else(missing)?,
            ),
            None => None,
        };
        let session = match selection.session {
            Some(id) => Some(self.agents.session(actor, company, id).await?),
            None => None,
        };
        let chosen = selection
            .machine
            .or_else(|| {
                session
                    .as_ref()
                    .map(|session| session.session.selection.machine_id)
            })
            .or_else(|| profile.as_ref().map(|profile| profile.selection.machine_id))
            .or_else(|| machines.first().map(|machine| machine.id));
        let (machine, runtime) = match chosen {
            Some(id) => (
                Some(self.machines.details(actor, company, id).await?),
                Some(self.agents.runtime(actor, company, id).await?),
            ),
            None => (None, None),
        };
        let conversation = self.conversation(company, &selection).await?;
        let sessions = match &conversation {
            Some(thread) => Some(
                self.agents
                    .sessions(
                        actor,
                        company,
                        &SessionAssociation::Conversation {
                            conversation_id: thread.id,
                        },
                    )
                    .await?,
            ),
            None => None,
        };
        let test = match selection.test {
            Some(id) => {
                let operation = self.agents.operation(actor, company, id).await?;
                if !matches!(
                    operation.operation.command(),
                    LocalCommand::RefreshCatalog {}
                ) {
                    return Err(AppError::BadRequest(
                        "Select a catalog connection test".into(),
                    ));
                }
                Some(operation)
            }
            None => None,
        };
        Ok(Html(pages::page(&pages::Page {
            company: &access.company,
            machines: &machines,
            pairings: &pairings,
            profiles: &profiles,
            native_agents: &native_agents,
            machine: machine.as_ref(),
            runtime: runtime.as_ref(),
            profile: profile.as_ref(),
            provider: selection.provider,
            model: selection.model.as_ref(),
            conversation: conversation.as_ref(),
            sessions: sessions.as_ref(),
            session: session.as_ref(),
            test: test.as_ref(),
            pairing: pairing.as_ref(),
        })))
    }
    async fn conversation(
        &self,
        company: Uuid,
        selection: &Selection,
    ) -> AppResult<Option<Thread>> {
        match (selection.channel, selection.conversation) {
            (Some(channel), Some(thread)) => self
                .threads
                .readable_thread(&self.viewer, company, channel, thread)
                .await?
                .ok_or_else(missing)
                .map(Some),
            (None, None) => Ok(None),
            _ => Err(AppError::BadRequest(
                "Choose a conversation from Mailbox".into(),
            )),
        }
    }
}

async fn page(
    workspace: Workspace,
    Path(company): Path<Uuid>,
    Query(selection): Query<Selection>,
) -> AppResult<Html<String>> {
    workspace.render(company, selection, None).await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PairForm {
    replace: Option<MachineId>,
}
async fn pair(
    workspace: Workspace,
    Path(company): Path<Uuid>,
    Form(form): Form<PairForm>,
) -> AppResult<Html<String>> {
    let pairing = workspace
        .machines
        .create_pairing(workspace.viewer.user_id, company, form.replace)
        .await?;
    workspace
        .render(company, Selection::default(), Some(pairing))
        .await
}
async fn cancel_pair(
    workspace: Workspace,
    Path((company, pair)): Path<(Uuid, PairingId)>,
) -> AppResult<Redirect> {
    workspace
        .machines
        .cancel_pairing(workspace.viewer.user_id, company, pair)
        .await?;
    Ok(redirect(company, ""))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChangeForm {
    change: MachineChange,
}
async fn change(
    workspace: Workspace,
    Path((company, machine)): Path<(Uuid, MachineId)>,
    Form(form): Form<ChangeForm>,
) -> AppResult<Redirect> {
    workspace
        .machines
        .change(workspace.viewer.user_id, company, machine, form.change)
        .await?;
    Ok(redirect(company, &format!("machine={machine}")))
}
async fn projects(
    workspace: Workspace,
    Path((company, machine)): Path<(Uuid, MachineId)>,
    Form(fields): Form<Vec<(String, String)>>,
) -> AppResult<Redirect> {
    if fields.len() > bounds::MAX_PROJECTS || fields.iter().any(|(name, _)| name != "project") {
        return Err(AppError::BadRequest("Invalid project selection".into()));
    }
    let projects = fields
        .into_iter()
        .map(|(_, value)| {
            CloudroomProjectId::try_from(value)
                .map_err(|_| AppError::BadRequest("Invalid project".into()))
        })
        .collect::<AppResult<Vec<_>>>()?;
    workspace
        .machines
        .set_projects(workspace.viewer.user_id, company, machine, &projects)
        .await?;
    Ok(redirect(company, &format!("machine={machine}")))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TestForm {
    request_id: OperationId,
}
async fn test(
    workspace: Workspace,
    Path((company, machine)): Path<(Uuid, MachineId)>,
    Form(form): Form<TestForm>,
) -> AppResult<Redirect> {
    workspace
        .agents
        .test_connection(workspace.viewer.user_id, company, machine, form.request_id)
        .await?;
    Ok(redirect(
        company,
        &format!("machine={machine}&test={}", form.request_id),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileForm {
    name: String,
    machine_id: MachineId,
    project: CloudroomProjectId,
    provider: LocalProvider,
    model: ModelId,
    reasoning: LocalReasoningLevel,
    permissions: LocalPermissions,
    agent_id: Option<String>,
}
impl ProfileForm {
    fn profile(self, company: Uuid, id: ProfileId) -> AppResult<LocalAgentProfile> {
        let agent_id = self
            .agent_id
            .filter(|value| !value.is_empty())
            .map(|value| {
                value
                    .parse::<Uuid>()
                    .ok()
                    .filter(|id| !id.is_nil())
                    .map(crate::entities::agent_capability::AgentId::new)
                    .ok_or_else(|| AppError::BadRequest("Choose a valid company agent".into()))
            })
            .transpose()?;
        Ok(LocalAgentProfile {
            id,
            company_id: company,
            name: self.name,
            agent_id,
            selection: ExecutionSelection {
                machine_id: self.machine_id,
                project_id: self.project,
                provider: self.provider,
                model: self.model,
                policy: ExecutionPolicy {
                    workspace: LocalWorkspace::ProjectDirectory,
                    permissions: self.permissions,
                    reasoning: self.reasoning,
                },
            },
        })
    }
}
async fn create_profile(
    workspace: Workspace,
    Path(company): Path<Uuid>,
    Form(form): Form<ProfileForm>,
) -> AppResult<Redirect> {
    let id = ProfileId::new(Uuid::new_v4()).map_err(|_| missing())?;
    workspace
        .machines
        .save_profile(workspace.viewer.user_id, &form.profile(company, id)?)
        .await?;
    Ok(redirect(company, &format!("profile={id}")))
}
async fn update_profile(
    workspace: Workspace,
    Path((company, id)): Path<(Uuid, ProfileId)>,
    Form(form): Form<ProfileForm>,
) -> AppResult<Redirect> {
    workspace
        .agents
        .profile(workspace.viewer.user_id, company, id)
        .await?
        .ok_or_else(missing)?;
    workspace
        .machines
        .save_profile(workspace.viewer.user_id, &form.profile(company, id)?)
        .await?;
    Ok(redirect(company, &format!("profile={id}")))
}
async fn delete_profile(
    workspace: Workspace,
    Path((company, id)): Path<(Uuid, ProfileId)>,
) -> AppResult<Redirect> {
    workspace
        .agents
        .delete_profile(workspace.viewer.user_id, company, id)
        .await?;
    Ok(redirect(company, ""))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartForm {
    request_id: OperationId,
    profile_id: ProfileId,
    conversation_id: Uuid,
    prompt: LocalPrompt,
}
async fn start(
    workspace: Workspace,
    Path(company): Path<Uuid>,
    Form(form): Form<StartForm>,
) -> AppResult<Redirect> {
    workspace
        .agents
        .start(StartSession {
            actor: workspace.viewer.user_id,
            company,
            request_id: form.request_id,
            profile_id: form.profile_id,
            conversation_id: form.conversation_id,
            prompt: form.prompt,
        })
        .await?;
    Ok(redirect(company, &format!("session={}", form.request_id)))
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum CommandKind {
    Send,
    Inspect,
    Stop,
    Archive,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ControlForm {
    request_id: OperationId,
    kind: CommandKind,
    prompt: Option<LocalPrompt>,
}
async fn control(
    workspace: Workspace,
    Path((company, session_id)): Path<(Uuid, SessionId)>,
    Form(form): Form<ControlForm>,
) -> AppResult<Redirect> {
    let command = match form.kind {
        CommandKind::Send => SessionCommand::Send {
            prompt: form
                .prompt
                .ok_or_else(|| AppError::BadRequest("Prompt required".into()))?,
        },
        CommandKind::Inspect => SessionCommand::Inspect,
        CommandKind::Stop => SessionCommand::Stop,
        CommandKind::Archive => SessionCommand::Archive,
    };
    workspace
        .agents
        .control(ControlSession {
            actor: workspace.viewer.user_id,
            company,
            request_id: form.request_id,
            session_id,
            command,
        })
        .await?;
    Ok(redirect(company, &format!("session={session_id}")))
}
fn redirect(company: Uuid, query: &str) -> Redirect {
    Redirect::to(&format!("/ui/companies/{company}/local-agents?{query}"))
}
fn missing() -> AppError {
    AppError::NotFound("Local agent resource".into())
}
