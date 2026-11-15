use super::*;
use crate::application::ports::channel::{ChannelPersistence, ChannelWrite};
use crate::{
    adapters::persistence::test_support::{OwnDatabase, own_database},
    use_cases::{
        company::{CompanyPersistence, CompanyWrite},
        user::UserPersistence,
    },
};
use std::sync::Arc;

pub(crate) struct Fixture {
    pub database: OwnDatabase,
    pub persistence: Arc<PostgresPersistence>,
    pub cases: Arc<LocalMachineUseCases>,
    pub owner: Uuid,
    pub admin: Uuid,
    pub member: Uuid,
    pub stranger: Uuid,
    pub company: Uuid,
    pub foreign: Uuid,
}

pub(crate) struct Candidate {
    pub claim: PairingClaim,
    pub secret: PairingClaimSecret,
    pub credential: MachineCredential,
}

impl Candidate {
    pub fn new(code: PairingCode) -> Self {
        let secret = PairingClaimSecret::generate().unwrap();
        let credential = MachineCredential::generate().unwrap();
        Self {
            claim: PairingClaim {
                code,
                installation_id: InstallationId::new(Uuid::new_v4()).unwrap(),
                name: "disposable test machine".into(),
                claim_verifier: secret.verifier(),
                credential_verifier: credential.verifier(),
            },
            secret,
            credential,
        }
    }
}

pub(crate) struct Paired {
    pub candidate: Candidate,
    pub receipt: PairingReceipt,
}

impl Fixture {
    pub async fn new() -> Option<Self> {
        let database = own_database().await?;
        let persistence = Arc::new(PostgresPersistence::new(database.pool.clone()));
        let mut accounts = Vec::new();
        for role in ["owner", "admin", "member", "stranger"] {
            let name = format!("{role}_{}", Uuid::new_v4().simple());
            let user = persistence
                .create_user(&name, &format!("{name}@example.test"), "hash")
                .await
                .unwrap();
            accounts.push(user.id);
        }
        let owner = accounts[0];
        let mut companies = Vec::new();
        for label in ["local", "foreign"] {
            companies.push(
                CompanyPersistence::create(
                    persistence.as_ref(),
                    owner,
                    CompanyWrite {
                        name: format!("Cloudroom {label}"),
                        slug: format!("{label}-{}", Uuid::new_v4().simple()),
                        ..Default::default()
                    },
                )
                .await
                .unwrap()
                .id,
            );
        }
        for (user, role) in [(accounts[1], "admin"), (accounts[2], "member")] {
            let mut tx = database.pool.begin().await.unwrap();
            sqlx::query("INSERT INTO company_members (id,company_id,user_id,role,access_level) VALUES ($1,$2,$3,$4,0)")
                .bind(Uuid::new_v4()).bind(companies[0]).bind(user).bind(role).execute(&mut *tx).await.unwrap();
            sqlx::query("INSERT INTO principals (id,company_id,kind,user_id,display_label) \
                SELECT $1,$2,'person',$3,$4 WHERE NOT EXISTS(SELECT 1 FROM principals WHERE company_id=$2 AND user_id=$3 AND kind='person')")
                .bind(Uuid::new_v4()).bind(companies[0]).bind(user).bind(role).execute(&mut *tx).await.unwrap();
            tx.commit().await.unwrap();
        }
        let cases = Arc::new(LocalMachineUseCases::new(
            persistence.clone(),
            persistence.clone(),
            "cloudroom-auth-test-key-distinct-32-bytes".into(),
        ));
        Some(Self {
            database,
            persistence,
            cases,
            owner,
            admin: accounts[1],
            member: accounts[2],
            stranger: accounts[3],
            company: companies[0],
            foreign: companies[1],
        })
    }

    pub async fn pair(&self, company: Uuid) -> Paired {
        let issued = self
            .cases
            .create_pairing(self.owner, company, None)
            .await
            .unwrap();
        let candidate = Candidate::new(issued.code);
        let receipt = self
            .cases
            .pair("127.0.0.1".parse().unwrap(), candidate.claim.clone())
            .await
            .unwrap();
        Paired { candidate, receipt }
    }

    pub async fn install_catalog(&self, paired: &Paired) -> CapabilitySnapshot {
        let machine = &paired.receipt.machine;
        let catalog = json!({"projects":["proj_test"],"providers":[{"provider":"codex","permission_modes":["full","auto","accept_edits"],"models":[{
            "model":"gpt-6-luna","reasoning_levels":["none","low","high"]
        }]}]});
        let mut tx = self.database.pool.begin().await.unwrap();
        sqlx::query("INSERT INTO local_machine_capabilities (company_id,machine_id,installation_id,sequence,catalog) VALUES ($1,$2,$3,1,$4)")
            .bind(machine.company_id).bind(machine.id.as_uuid()).bind(machine.installation_id.as_uuid()).bind(catalog).execute(&mut *tx).await.unwrap();
        sqlx::query(
            "UPDATE local_machines SET capabilities_sequence=1 WHERE company_id=$1 AND id=$2",
        )
        .bind(machine.company_id)
        .bind(machine.id.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        self.persistence
            .capabilities(
                machine.company_id,
                machine.id,
                EventSequence::try_from(1).unwrap(),
            )
            .await
            .unwrap()
            .unwrap()
    }

    pub fn profile(&self, machine: MachineId) -> LocalAgentProfile {
        LocalAgentProfile {
            agent_id: None,
            id: ProfileId::new(Uuid::new_v4()).unwrap(),
            company_id: self.company,
            name: "test profile".into(),
            selection: ExecutionSelection {
                machine_id: machine,
                project_id: CloudroomProjectId::try_from("proj_test".to_owned()).unwrap(),
                provider: LocalProvider::Codex,
                model: ModelId::try_from("gpt-6-luna".to_owned()).unwrap(),
                policy: ExecutionPolicy {
                    workspace: LocalWorkspace::ProjectDirectory,
                    permissions: LocalPermissions::Full,
                    reasoning: LocalReasoningLevel::Low,
                },
            },
        }
    }

    pub async fn conversation(&self, company: Uuid) -> Uuid {
        let channel = ChannelPersistence::create(
            self.persistence.as_ref(),
            company,
            ChannelWrite {
                name: "local fixture".into(),
                slug: format!("local-{}", Uuid::new_v4().simple()),
                enabled: true,
                add_3rd_party: true,
                created_by: Some(crate::entities::creation::CreationProvenance::user(
                    self.owner,
                )),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        let thread = Uuid::new_v4();
        sqlx::query("INSERT INTO threads (id,company_id,channel_id,subject) VALUES ($1,$2,$3,'local fixture')")
            .bind(thread).bind(company).bind(channel).execute(&self.database.pool).await.unwrap();
        thread
    }
}

pub(crate) struct SessionSetup {
    pub(crate) profile: LocalAgentProfile,
    pub(crate) id: SessionId,
    pub(crate) conversation: Uuid,
    pub(crate) selection: Value,
}

pub(crate) async fn prepare_session(f: &Fixture, paired: &Paired) -> SessionSetup {
    let snapshot = f.install_catalog(paired).await;
    assert_eq!(
        snapshot.installation_id,
        paired.receipt.machine.installation_id
    );
    f.cases
        .set_projects(
            f.owner,
            f.company,
            paired.receipt.machine.id,
            &snapshot.catalog.projects,
        )
        .await
        .unwrap();
    let profile = f.profile(paired.receipt.machine.id);
    f.cases.save_profile(f.owner, &profile).await.unwrap();
    let session = SessionId::new(Uuid::new_v4()).unwrap();
    let conversation = f.conversation(f.company).await;
    let selection = encode(&profile.selection).unwrap();
    sqlx::query("INSERT INTO local_agent_sessions (company_id,id,machine_id,installation_id,profile_id,selection,conversation_id,thread_id) VALUES ($1,$2,$3,$4,$5,$6,$7,'thr_frozen')")
        .bind(f.company).bind(session.as_uuid()).bind(paired.receipt.machine.id.as_uuid()).bind(paired.receipt.machine.installation_id.as_uuid())
        .bind(profile.id.as_uuid()).bind(selection.clone()).bind(conversation).execute(&f.database.pool).await.unwrap();
    SessionSetup {
        profile,
        id: session,
        conversation,
        selection,
    }
}

pub(crate) async fn native_mcp_binding(
    p: &PostgresPersistence,
    tenant: Uuid,
    actor: Uuid,
    endpoint: &str,
) -> crate::entities::agent_capability::AgentToolBinding {
    use crate::application::agent::{ActionEffect, ActionRecovery, ApprovedActionPolicy};
    use crate::application::workflow::{mcp::*, publication::ToolSnapshot, registry::ToolContract};
    use crate::domain::workflow::TypeName;
    use crate::entities::{agent_capability::*, mcp::*};
    use crate::use_cases::mcp::*;
    let company = CompanyId::new(tenant);
    let schema = json!({"type":"object","required":["query"],"additionalProperties":false,"properties":{"query":{"type":"string","maxLength":128}}});
    let connection = p
        .create_mcp_connection(
            tenant,
            McpConnectionWrite {
                slug: "native-crm".into(),
                endpoint: endpoint.to_owned().try_into().unwrap(),
                enabled: true,
                auth: McpAuth::None,
                discovered_tools: vec![McpDiscoveredTool {
                    name: "lookup".to_owned().try_into().unwrap(),
                    description: "Lookup".into(),
                    input_schema: schema.clone(),
                    output_schema: Some(json!({"type":"object"})),
                }],
                tool_grants: vec!["lookup".to_owned().try_into().unwrap()],
            },
        )
        .await
        .unwrap();
    let reviewed = p
        .register_mcp_operation(
            company,
            WorkflowActor::authenticated(actor).unwrap(),
            McpOperationWrite {
                connection: connection.id,
                catalog_revision: connection.revision,
                expected_version: None,
                operation: McpOperation {
                    tool: ToolSnapshot {
                        company_id: company,
                        workflow: None,
                        contract: ToolContract {
                            name: TypeName::parse("lookup").unwrap(),
                            connection: None,
                            input_schema: schema,
                            output_schema: json!({"type":"object"}),
                        },
                        policy: ApprovedActionPolicy {
                            capability: TypeName::parse("mcp.lookup").unwrap(),
                            policy_revision: 1,
                            effect: ActionEffect::Read,
                            recovery: ActionRecovery::Reconcile,
                        },
                    },
                    approval_required: true,
                },
            },
        )
        .await
        .unwrap();
    AgentToolBinding {
        id: AgentBindingId::new(Uuid::new_v4()),
        alias: AgentToolAlias::parse("crm.lookup").unwrap(),
        source: AgentBindingSource::Mcp {
            connection: connection.id,
            operation: TypeName::parse("lookup").unwrap(),
            reviewed_version: reviewed,
            remote_name: "lookup".to_owned().try_into().unwrap(),
        },
    }
}

#[path = "native_mcp_workflow_test_support.rs"]
pub(crate) mod mcp_workflow;
