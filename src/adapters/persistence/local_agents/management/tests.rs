use super::*;
use crate::adapters::persistence::local_agents::test_support::{Fixture, Paired};
use crate::application::local_agents::protocol::{ConnectionIssue, LocalProtocolStore};

struct Setup {
    f: Fixture,
    paired: Paired,
    profile: LocalAgentProfile,
    conversation: Uuid,
}
impl Setup {
    async fn new() -> Self {
        let f = Fixture::new()
            .await
            .expect("management tests require PostgreSQL");
        let paired = f.pair(f.company).await;
        f.install_catalog(&paired).await;
        let profile = f.profile(paired.receipt.machine.id);
        f.cases
            .set_projects(
                f.owner,
                f.company,
                profile.selection.machine_id,
                std::slice::from_ref(&profile.selection.project_id),
            )
            .await
            .unwrap();
        f.cases.save_profile(f.owner, &profile).await.unwrap();
        let conversation = f.conversation(f.company).await;
        Self {
            f,
            paired,
            profile,
            conversation,
        }
    }
    fn request(&self) -> StartSession {
        StartSession {
            actor: self.f.owner,
            company: self.f.company,
            request_id: OperationId::new(Uuid::now_v7()).unwrap(),
            profile_id: self.profile.id,
            conversation_id: self.conversation,
            prompt: LocalPrompt::try_from("explicit task".to_owned()).unwrap(),
        }
    }
    async fn start(&self) -> SessionId {
        let request = self.request();
        self.f.persistence.start_session(&request).await.unwrap();
        SessionId::new(request.request_id.as_uuid()).unwrap()
    }
    async fn map_thread(&self, id: SessionId) {
        // This metadata fixture exercises read/control authority, not native execution.
        sqlx::query("UPDATE local_agent_sessions SET thread_id='thread_management' WHERE company_id=$1 AND id=$2")
            .bind(self.f.company).bind(id.as_uuid()).execute(&self.f.database.pool).await.unwrap();
    }
    fn control(&self, session: SessionId, command: SessionCommand) -> ControlSession {
        ControlSession {
            actor: self.f.owner,
            company: self.f.company,
            request_id: OperationId::new(Uuid::now_v7()).unwrap(),
            session_id: session,
            command,
        }
    }
}

#[tokio::test]
async fn cloudroom_management_competing_starts_freeze_one_session_and_replay_before_profile_edits()
{
    let s = Setup::new().await;
    let request = s.request();
    let p = &s.f.persistence;
    let (a, b) = tokio::join!(p.start_session(&request), p.start_session(&request));
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(a.origin, OperationOrigin::Direct);
    assert_eq!(a.operation.payload_digest(), b.operation.payload_digest());
    assert_eq!(a.operation.deadline(), b.operation.deadline());
    let counts:(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM local_agent_sessions),(SELECT count(*) FROM local_agent_operations)")
        .fetch_one(&s.f.database.pool).await.unwrap();
    assert_eq!(counts, (1, 1));
    let mut changed = s.profile.clone();
    changed.selection.policy.reasoning = LocalReasoningLevel::High;
    s.f.cases.save_profile(s.f.owner, &changed).await.unwrap();
    let replay = p.start_session(&request).await.unwrap();
    assert_eq!(
        a.operation.payload_digest(),
        replay.operation.payload_digest()
    );
    assert!(
        matches!(replay.operation.command(),LocalCommand::Create{selection,..} if selection==&s.profile.selection)
    );
    let changed_request = StartSession {
        prompt: LocalPrompt::try_from("changed task".to_owned()).unwrap(),
        ..request
    };
    assert!(matches!(
        p.start_session(&changed_request).await,
        Err(AppError::Conflict(_))
    ));
}

#[tokio::test]
async fn cloudroom_management_private_and_foreign_session_reads_and_every_control_fail_closed() {
    let s = Setup::new().await;
    let id = s.start().await;
    s.map_thread(id).await;
    sqlx::query("UPDATE channels SET access_mode='allowlist' WHERE company_id=$1 AND id=(SELECT channel_id FROM threads WHERE company_id=$1 AND id=$2)")
        .bind(s.f.company).bind(s.conversation).execute(&s.f.database.pool).await.unwrap();
    let p = &s.f.persistence;
    assert!(p.read_session(s.f.owner, s.f.company, id).await.is_ok());
    for (company, actor) in [
        (s.f.company, s.f.admin),
        (s.f.foreign, s.f.owner),
        (s.f.company, s.f.member),
    ] {
        assert!(matches!(
            p.read_session(actor, company, id).await,
            Err(AppError::NotFound(_))
        ));
        for command in [
            SessionCommand::Send {
                prompt: LocalPrompt::try_from("forged".to_owned()).unwrap(),
            },
            SessionCommand::Inspect,
            SessionCommand::Stop,
            SessionCommand::Archive,
        ] {
            let request = ControlSession {
                company,
                actor,
                ..s.control(id, command)
            };
            assert!(matches!(
                p.control_session(&request).await,
                Err(AppError::NotFound(_))
            ));
        }
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM local_agent_operations")
        .fetch_one(&s.f.database.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    assert!(matches!(
        p.delete_profile(s.f.owner, s.f.foreign, s.profile.id).await,
        Err(AppError::NotFound(_))
    ));
    assert!(matches!(
        p.delete_profile(s.f.owner, s.f.company, s.profile.id).await,
        Err(AppError::Conflict(_))
    ));
}

#[tokio::test]
async fn cloudroom_management_controls_keep_disabled_headroom_and_stable_request_actor() {
    let s = Setup::new().await;
    let id = s.start().await;
    s.map_thread(id).await;
    s.f.cases
        .change(
            s.f.owner,
            s.f.company,
            s.profile.selection.machine_id,
            MachineChange::Disable,
        )
        .await
        .unwrap();
    let p = &s.f.persistence;
    let send = s.control(
        id,
        SessionCommand::Send {
            prompt: LocalPrompt::try_from("new effect".to_owned()).unwrap(),
        },
    );
    assert!(matches!(
        p.control_session(&send).await,
        Err(AppError::Conflict(_))
    ));
    for command in [
        SessionCommand::Inspect,
        SessionCommand::Stop,
        SessionCommand::Archive,
    ] {
        let request = s.control(id, command);
        let one = p.control_session(&request).await.unwrap();
        let two = p.control_session(&request).await.unwrap();
        assert_eq!(one.operation.id(), two.operation.id());
        let foreign_actor = ControlSession {
            actor: s.f.admin,
            ..request
        };
        assert!(matches!(
            p.control_session(&foreign_actor).await,
            Err(AppError::NotFound(_))
        ));
    }
}

#[tokio::test]
async fn cloudroom_management_recent_output_is_bounded_and_usage_remains_unavailable() {
    let s = Setup::new().await;
    let id = s.start().await;
    s.map_thread(id).await;
    let p = &s.f.persistence;
    for _ in 0..10 {
        let operation = p
            .control_session(&s.control(id, SessionCommand::Inspect))
            .await
            .unwrap();
        sqlx::query("UPDATE local_agent_operations SET output=$3 WHERE company_id=$1 AND id=$2")
            .bind(s.f.company)
            .bind(operation.operation.id().as_uuid())
            .bind("é".repeat(MAX_RESULT_BYTES / 2))
            .execute(&s.f.database.pool)
            .await
            .unwrap();
    }
    let details = p.read_session(s.f.owner, s.f.company, id).await.unwrap();
    assert!(details.recent.truncated);
    assert_eq!(details.recent.entries.len(), SESSION_OPERATION_PAGE);
    assert!(
        details
            .recent
            .entries
            .iter()
            .all(|view| view.measured_usage.is_none())
    );
    assert_eq!(
        details
            .recent
            .entries
            .iter()
            .map(|view| view.output.as_ref().map_or(0, |out| out.as_str().len()))
            .sum::<usize>(),
        MAX_RESULT_BYTES
    );
    assert!(
        details
            .recent
            .entries
            .iter()
            .skip(1)
            .all(|view| view.output_truncated)
    );
}

#[tokio::test]
async fn cloudroom_management_profile_pages_are_complete_without_cross_company_rows() {
    let s = Setup::new().await;
    sqlx::query("INSERT INTO local_agent_profiles(company_id,id,machine_id,name,selection) SELECT $1,gen_random_uuid(),$2,'page profile '||number::text,$3 FROM generate_series(1,101) AS number")
        .bind(s.f.company).bind(s.profile.selection.machine_id.as_uuid()).bind(encode(&s.profile.selection).unwrap()).execute(&s.f.database.pool).await.unwrap();
    let p = &s.f.persistence;
    let first = p.list_profiles(s.f.owner, s.f.company, None).await.unwrap();
    assert_eq!(first.entries.len(), MAX_PAGE_RECORDS);
    assert!(first.truncated);
    let next = p
        .list_profiles(
            s.f.owner,
            s.f.company,
            Some(first.entries.last().unwrap().id),
        )
        .await
        .unwrap();
    assert_eq!(next.entries.len(), 2);
    assert!(!next.truncated);
    let ids = first
        .entries
        .iter()
        .chain(&next.entries)
        .map(|entry| entry.id)
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(ids.len(), 102);
    assert!(
        p.list_profiles(s.f.owner, s.f.foreign, None)
            .await
            .unwrap()
            .entries
            .is_empty()
    );
}

#[tokio::test]
async fn cloudroom_management_connection_diagnostics_are_fenced_and_do_not_claim_provider_execution()
 {
    let s = Setup::new().await;
    let p = &s.f.persistence;
    let machine = &s.paired.receipt.machine;
    let first = p
        .acquire_connection(
            s.paired.candidate.credential.verifier(),
            machine.installation_id,
            Uuid::new_v4(),
        )
        .await
        .unwrap()
        .unwrap();
    assert!(
        p.record_connection_issue(first, ConnectionIssue::IncompatibleProtocol)
            .await
            .unwrap()
    );
    assert!(
        !p.record_connection_issue(first, ConnectionIssue::ConnectionFailed)
            .await
            .unwrap()
    );
    assert_eq!(
        p.machine_runtime(s.f.owner, s.f.company, machine.id)
            .await
            .unwrap()
            .last_issue,
        Some(ConnectionIssue::IncompatibleProtocol)
    );
    let next = p
        .acquire_connection(
            s.paired.candidate.credential.verifier(),
            machine.installation_id,
            Uuid::new_v4(),
        )
        .await
        .unwrap()
        .unwrap();
    assert!(
        !p.record_connection_issue(first, ConnectionIssue::IncompatibleProtocol)
            .await
            .unwrap()
    );
    let runtime = p
        .machine_runtime(s.f.owner, s.f.company, machine.id)
        .await
        .unwrap();
    assert!(runtime.connected);
    assert!(!runtime.reconciled);
    assert!(runtime.last_issue.is_none());
    let request = OperationId::new(Uuid::now_v7()).unwrap();
    let test = p
        .refresh_catalog(s.f.owner, s.f.company, machine.id, request)
        .await
        .unwrap();
    assert!(matches!(test.command(), LocalCommand::RefreshCatalog {}));
    assert!(test.outcome().is_none());
    assert_eq!(
        test.payload_digest(),
        p.refresh_catalog(s.f.owner, s.f.company, machine.id, request)
            .await
            .unwrap()
            .payload_digest()
    );
    p.release_connection(next).await.unwrap();
}

#[tokio::test]
#[ignore = "explicit browser fixture output"]
async fn dump_cloudroom_management_browser_fixtures() {
    use crate::adapters::http::pages::{self, local_agents};
    use crate::{
        application::ports::thread::ThreadPersistence, use_cases::company::CompanyPersistence,
    };
    let root = std::path::PathBuf::from(
        std::env::var("CLOUDROOM_MANAGEMENT_BROWSER_FIXTURES").expect("fixture directory"),
    );
    std::fs::create_dir_all(&root).unwrap();
    let s = Setup::new().await;
    let id = s.start().await;
    s.map_thread(id).await;
    let p = &s.f.persistence;
    let company = CompanyPersistence::get_by_id(p.as_ref(), s.f.company)
        .await
        .unwrap()
        .unwrap();
    let mut conversation = p.get_thread_by_id(s.conversation).await.unwrap().unwrap();
    conversation.subject = "Conversation <img src=x onerror=alert(1)>".into();
    let mut profile = s.profile.clone();
    profile.name = "Profile \"<img src=x onerror=alert(1)>".into();
    let profiles = RecordPage {
        entries: vec![profile.clone()],
        truncated: false,
    };
    let machine =
        s.f.cases
            .details(s.f.owner, s.f.company, s.profile.selection.machine_id)
            .await
            .unwrap();
    let runtime = p
        .machine_runtime(s.f.owner, s.f.company, s.profile.selection.machine_id)
        .await
        .unwrap();
    let mut session = p.read_session(s.f.owner, s.f.company, id).await.unwrap();
    session.recent.entries[0].output =
        Some(LocalOutput::try_from("Result <img src=x onerror=alert(1)>".to_owned()).unwrap());
    session.recent.entries[0].observed_turn_state = Some(TurnState::WaitingForInput);
    let view = local_agents::Page {
        native_agents: &RecordPage {
            entries: vec![],
            truncated: false,
        },
        company: &company,
        machines: std::slice::from_ref(&machine.machine),
        pairings: &[],
        profiles: &profiles,
        machine: Some(&machine),
        runtime: Some(&runtime),
        profile: Some(&profile),
        provider: None,
        model: None,
        conversation: Some(&conversation),
        sessions: None,
        session: Some(&session),
        test: None,
        pairing: None,
    };
    std::fs::write(root.join("page.html"), local_agents::page(&view)).unwrap();
    std::fs::write(root.join("app.js"), pages::application_javascript()).unwrap();
    std::fs::write(root.join("theme-init.js"), pages::theme_init_javascript()).unwrap();
    std::fs::write(root.join("metadata.json"),json!({"company":s.f.company,"machine":profile.selection.machine_id,"profile":profile.id,"session":id,"conversation":s.conversation}).to_string()).unwrap();
    write_profile_navigation(&s, &root, &view).await;
    write_agent_permissions_fixture(&s, &root, &view).await;
}

async fn write_profile_navigation(
    s: &Setup,
    root: &std::path::Path,
    view: &crate::adapters::http::pages::local_agents::Page<'_>,
) {
    use crate::adapters::http::pages::local_agents;
    let p = &s.f.persistence;
    sqlx::query("INSERT INTO local_agent_profiles(company_id,id,machine_id,name,selection) SELECT $1,gen_random_uuid(),$2,'Paging profile '||number::text,$3 FROM generate_series(1,101) AS number")
        .bind(s.f.company).bind(s.profile.selection.machine_id.as_uuid()).bind(encode(&s.profile.selection).unwrap()).execute(&s.f.database.pool).await.unwrap();
    let first = p.list_profiles(s.f.owner, s.f.company, None).await.unwrap();
    let cursor = first.entries.last().unwrap().id;
    let second = p
        .list_profiles(s.f.owner, s.f.company, Some(cursor))
        .await
        .unwrap();
    let chosen = second
        .entries
        .iter()
        .find(|profile| profile.id != s.profile.id)
        .unwrap();
    for (name, profiles, selected) in [
        ("profiles-first.html", &first, None),
        ("profiles-next.html", &second, None),
        ("profiles-selected.html", &first, Some(chosen)),
    ] {
        let page = local_agents::Page {
            profiles,
            profile: selected,
            ..*view
        };
        std::fs::write(root.join(name), local_agents::page(&page)).unwrap();
    }
    std::fs::write(
        root.join("profiles-navigation.json"),
        json!({"company":s.f.company,"channel":view.conversation.unwrap().channel_id,
        "conversation":view.conversation.unwrap().id,"cursor":cursor,"profile":chosen.id,"name":chosen.name})
        .to_string(),
    )
    .unwrap();
}

#[path = "native_mcp_tests.rs"]
mod native_mcp_tests;
#[path = "native_tests.rs"]
mod native_tests;

async fn write_agent_permissions_fixture(
    s: &Setup,
    root: &std::path::Path,
    view: &crate::adapters::http::pages::local_agents::Page<'_>,
) {
    use crate::adapters::http::pages::local_agents;
    use crate::application::agent::{
        AgentPublicationKey, AgentPublicationScope, AgentPublications, PublishAgent,
    };
    use crate::entities::agent_capability::AgentId;
    use crate::use_cases::agent::{AgentPersistence, AgentWrite};
    let p = s.f.persistence.as_ref();
    let agent = AgentPersistence::create(
        p,
        s.f.company,
        AgentWrite {
            name: "Auto native agent".into(),
            slug: "auto-native-agent".into(),
            native_permissions: LocalPermissions::Auto,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    p.publish_agent(PublishAgent {
        scope: AgentPublicationScope {
            company: CompanyId::new(s.f.company),
            actor: WorkflowActor::authenticated(s.f.owner).unwrap(),
            agent: AgentId::new(agent.id),
        },
        expected_draft_revision: agent.draft_revision,
        expected_active_revision: None,
        key: AgentPublicationKey::parse("browser-auto-agent").unwrap(),
    })
    .await
    .unwrap();
    sqlx::query("UPDATE local_machine_capabilities SET catalog=jsonb_set(catalog,'{providers,0,permission_modes}','[\"auto\"]') WHERE company_id=$1")
        .bind(s.f.company).execute(&s.f.database.pool).await.unwrap();
    let machine =
        s.f.cases
            .details(s.f.owner, s.f.company, s.profile.selection.machine_id)
            .await
            .unwrap();
    let choices = RecordPage {
        entries: vec![NativeAgentChoice {
            id: AgentId::new(agent.id),
            name: agent.name,
        }],
        truncated: false,
    };
    let native_view = local_agents::Page {
        machine: Some(&machine),
        native_agents: &choices,
        ..*view
    };
    std::fs::write(
        root.join("profiles-agent-permissions.html"),
        local_agents::page(&native_view),
    )
    .unwrap();
    std::fs::write(
        root.join("profile-agent-permissions.json"),
        json!({"agent":agent.id,"fallback":"full"}).to_string(),
    )
    .unwrap();
}
