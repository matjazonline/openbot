//! Opt-in real native provider matrix; never part of credential-free CI execution.
use super::*;
use crate::application::local_agents::LocalMachineUseCases;
use futures::FutureExt;
#[path = "local_agent_native_recovery_tests.rs"]
mod recovery;
use recovery::initial_native;
#[path = "local_agent_native_interaction_tests.rs"]
mod interactions;

struct NativeRig {
    s: Setup,
    cases: Arc<LocalMachineUseCases>,
    server: TestTlsServer,
    peer: Peer,
    workspace: PathBuf,
    marker: String,
    provider: LocalProvider,
    evidence: PathBuf,
}
struct NativeInitial {
    session: SessionId,
    thread: CloudroomThreadId,
    before: Value,
    after: Value,
}
async fn prepare_native(provider: LocalProvider) -> NativeRig {
    assert_eq!(
        std::env::var("OPENBOT_CLOUDROOM_NATIVE_SMOKE").as_deref(),
        Ok("1"),
        "native acceptance needs explicit opt-in"
    );
    let source = PathBuf::from(std::env::var("CLOUDROOM_SOURCE").expect("prepared pinned source"));
    let artifact =
        PathBuf::from(std::env::var("CLOUDROOM_PLUGIN_ARCHIVE").expect("verified plugin archive"));
    let evidence = PathBuf::from(
        std::env::var("CLOUDROOM_NATIVE_EVIDENCE").expect("retained native evidence directory"),
    );
    std::fs::create_dir_all(&evidence).unwrap();
    let directory = std::env::temp_dir().join(format!("openbot-native-e2e-{}", Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    let workspace = directory.join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let marker = format!("OPENBOT_NATIVE_{}", Uuid::new_v4().simple());
    std::fs::write(workspace.join("marker.txt"), format!("{marker}\n")).unwrap();
    let prompt = format!(
        "Authorized disposable local file smoke: use your shell tool once to run exactly cat {} and reply with the file's exact contents. Do not edit files, use other commands, access network, or start subagents.",
        workspace.join("marker.txt").display()
    );
    let s = Setup::with_inputs(|inputs, _| inputs["prompt"] = json!({"literal":prompt})).await;
    let p = s.fixture.persistence.clone();
    let cases = Arc::new(LocalMachineUseCases::new(
        Arc::new(p.clone()),
        Arc::new(p.clone()),
        "native-e2e-test-authority-unique-key-32".into(),
    ));
    let issued = cases
        .create_pairing(
            s.fixture.target.actor.user_id(),
            s.scope.company.as_uuid(),
            Some(s.profile.selection.machine_id),
        )
        .await
        .unwrap();
    let extra =
        crate::adapters::http::routes::local_machines::public_router().with_state(cases.clone());
    let server = TestTlsServer::with_routes(Arc::new(p.clone()), extra).await;
    let encoded = base64::engine::general_purpose::STANDARD.encode(server.certificate.as_ref());
    std::fs::write(
        directory.join("ca.pem"),
        format!("-----BEGIN CERTIFICATE-----\n{encoded}\n-----END CERTIFICATE-----\n"),
    )
    .unwrap();
    let provider_name = serde_json::to_value(provider)
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned();
    let provider_evidence = evidence.join(&provider_name);
    std::fs::create_dir_all(&provider_evidence).unwrap();
    std::fs::write(directory.join("config.json"),serde_json::to_vec(&json!({"directory":directory,"workspace":workspace,
        "source":source,"artifact":artifact,"origin":server.origin,"pairingCode":issued.code.expose_secret(),
        "provider":provider_name,"opencodeBin":std::env::var("CLOUDROOM_OPENCODE_BIN").ok(),"evidence":provider_evidence})).unwrap()).unwrap();
    std::fs::set_permissions(
        directory.join("config.json"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();

    let peer = Peer::spawn_script(directory, "native-peer.js");
    NativeRig {
        s,
        cases,
        server,
        peer,
        workspace,
        marker,
        provider,
        evidence: provider_evidence,
    }
}
async fn configure_native(rig: &mut NativeRig) {
    let ready = rig.peer.event_timeout("ready", 180).await;
    let project =
        CloudroomProjectId::try_from(ready["project"].as_str().unwrap().to_owned()).unwrap();
    let s = &mut rig.s;
    let name = format!(
        "Disposable {} acceptance",
        serde_json::to_value(rig.provider)
            .unwrap()
            .as_str()
            .unwrap()
    );
    let machine = rig
        .cases
        .machines(
            s.fixture.target.actor.user_id(),
            s.scope.company.as_uuid(),
            None,
        )
        .await
        .unwrap()
        .into_iter()
        .find(|machine| machine.name == name)
        .expect("use the actual installation enrolled by the production plugin");
    assert_eq!(machine.state, MachineState::Enabled);
    s.profile.selection.machine_id = machine.id;
    let details = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            let details = rig
                .cases
                .details(
                    s.fixture.target.actor.user_id(),
                    s.scope.company.as_uuid(),
                    s.profile.selection.machine_id,
                )
                .await
                .unwrap();
            if details
                .capabilities
                .as_ref()
                .is_some_and(|cap| cap.catalog.projects.contains(&project))
            {
                break details;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
    rig.cases
        .set_projects(
            s.fixture.target.actor.user_id(),
            s.scope.company.as_uuid(),
            s.profile.selection.machine_id,
            std::slice::from_ref(&project),
        )
        .await
        .unwrap();
    let catalog = details.capabilities.as_ref().unwrap();
    let models = &catalog
        .catalog
        .providers
        .iter()
        .find(|item| item.provider == rig.provider)
        .expect("selected real provider must advertise a catalog")
        .models;
    let selected = models
        .iter()
        .find(|item| match rig.provider {
            LocalProvider::Codex => item.model.as_str() == "gpt-6-luna",
            LocalProvider::OpenCode => item.model.as_str() == "openai/gpt-6-luna",
            LocalProvider::ClaudeCode => item.model.as_str().contains("haiku"),
        })
        .expect("explicit bounded smoke model must be advertised");
    s.profile.selection.project_id = project;
    s.profile.selection.provider = rig.provider;
    s.profile.selection.model = selected.model.clone();
    s.profile.selection.policy.reasoning = if selected
        .reasoning_levels
        .contains(&LocalReasoningLevel::Low)
    {
        LocalReasoningLevel::Low
    } else {
        *selected.reasoning_levels.first().unwrap()
    };
    rig.cases
        .save_profile(s.fixture.target.actor.user_id(), &s.profile)
        .await
        .unwrap();
}
async fn follow_native(rig: &mut NativeRig, initial: &NativeInitial) {
    let s = &rig.s;
    let p = &s.fixture.persistence;
    let marker = format!("OPENBOT_FOLLOW_{}", Uuid::new_v4().simple());
    let request=ControlSession {actor:s.fixture.target.actor.user_id(),company:s.scope.company.as_uuid(),request_id:OperationId::new(Uuid::now_v7()).unwrap(),session_id:initial.session,
        command:SessionCommand::Send {prompt:LocalPrompt::try_from(format!("Reply exactly {marker}. Do not use tools, change files, access network, or start subagents.")).unwrap()}};
    p.control_session(&request).await.unwrap();
    native_outcome(s, request.request_id, OperationOutcome::Succeeded).await;
    let details = p
        .read_operation(request.actor, request.company, request.request_id)
        .await
        .unwrap();
    assert!(details.output.as_ref().unwrap().as_str().contains(&marker));
    rig.peer
        .command(json!({"command":"ui","thread":initial.thread,"text":marker}))
        .await;
    rig.peer.event_timeout("ui", 120).await;
}
async fn stop_native(
    rig: &mut NativeRig,
    initial: &NativeInitial,
    expected_requests: usize,
) -> Value {
    let s = &rig.s;
    let p = &s.fixture.persistence;
    let active = ControlSession {
        actor: s.fixture.target.actor.user_id(), company: s.scope.company.as_uuid(),
        request_id: OperationId::new(Uuid::now_v7()).unwrap(), session_id: initial.session,
        command: SessionCommand::Send { prompt: LocalPrompt::try_from(
            "Authorized disposable cancellation smoke: use your shell tool once to run exactly sleep 20. Do not read or change files, use network, or start subagents.".to_owned()).unwrap() },
    };
    p.control_session(&active).await.unwrap();
    rig.peer
        .command(
            json!({"command":"executing","thread":initial.thread,"operation":active.request_id}),
        )
        .await;
    let executing = rig.peer.event_timeout("executing", 45).await;
    let mut controls = Vec::new();
    for command in [SessionCommand::Stop, SessionCommand::Archive] {
        let control = ControlSession {
            actor: active.actor,
            company: active.company,
            request_id: OperationId::new(Uuid::now_v7()).unwrap(),
            session_id: initial.session,
            command,
        };
        p.control_session(&control).await.unwrap();
        let view = interactions::settled(rig, control.request_id).await;
        let outcome = view.operation.outcome().unwrap();
        assert!(matches!(
            outcome,
            OperationOutcome::Succeeded | OperationOutcome::Uncertain
        ));
        rig.peer
            .command(json!({"command":"journal","operation":control.request_id}))
            .await;
        assert_eq!(rig.peer.event("journal").await["sdkCalls"], 1);
        controls.push(json!({"command":control.command,"outcome":outcome,"sdkCalls":1}));
        if matches!(control.command, SessionCommand::Stop) {
            native_outcome(s, active.request_id, OperationOutcome::Cancelled).await;
        }
    }
    rig.peer
        .command(json!({"command":"snapshot","thread":initial.thread}))
        .await;
    let mut final_state = rig.peer.event("snapshot").await;
    assert_eq!(
        final_state["archived"], true,
        "actual native archive must be witnessed even if the SDK reply was uncertain"
    );
    final_state["controls"] = json!(controls);
    assert_eq!(final_state["requests"], expected_requests);
    assert_eq!(
        final_state["turns"].as_array().unwrap().len(),
        expected_requests
    );
    assert_eq!(
        final_state["terminals"].as_array().unwrap().len(),
        expected_requests
    );
    assert!(
        final_state["terminals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["turn"] == executing["turn"] && item["status"] == "interrupted")
    );
    final_state
}
async fn verify_native(rig: &mut NativeRig) -> Value {
    configure_native(rig).await;
    let initial = initial_native(rig).await;
    follow_native(rig, &initial).await;
    let pending_permission = interactions::begin_permission(rig, &initial).await;
    let interaction = interactions::finish_permission(rig, &initial, pending_permission).await;
    let final_state = stop_native(rig, &initial, 6).await;
    let logged_out = interactions::logged_out(rig).await;
    json!({"status":"PASS","platform":std::env::consts::OS,"provider":rig.provider,"selection":rig.s.profile.selection,
        "session":initial.session,"thread":initial.thread,"beforeRestart":initial.before,"afterRestart":initial.after,"final":final_state,
        "nativeHumanPermission":interaction,"isolatedLoggedOutProvider":logged_out,
        "realPairingHttp":true,"productionPlugin":true,"actualWorkflowWorkers":2,"actualUi":true,"fileAccess":true,
        "outputRecoveredAfterNativeOutputBeforeServerReceipt":true,"explicitThreadControlsAdmitted":true,"nativeThreadArchived":true,"activeNativeStopMatchedInterruptedTurn":true,"nativeStopIsNotAnAtomicTargetedTurnGuarantee":true,"linux":"NOT EXERCISED on this host"})
}
async fn close_native(mut rig: NativeRig) -> std::process::ExitStatus {
    if rig.peer.child.try_wait().unwrap().is_none() {
        rig.peer.command(json!({"command":"close"})).await;
    }
    let status = tokio::time::timeout(std::time::Duration::from_secs(90), rig.peer.child.wait())
        .await
        .unwrap()
        .unwrap();
    rig.server.close().await;
    assert_eq!(
        std::fs::read_to_string(rig.workspace.join("marker.txt")).unwrap(),
        format!("{}\n", rig.marker),
        "removal preserves workspace before disposable-fixture cleanup"
    );
    status
}
async fn native_case(provider: LocalProvider) {
    // Keep the external runtime fixture's future off the libtest stack.
    let mut rig = Box::pin(prepare_native(provider)).await;
    let evidence = rig.evidence.join("result.json");
    let result = std::panic::AssertUnwindSafe(Box::pin(verify_native(&mut rig)))
        .catch_unwind()
        .await;
    let status = Box::pin(close_native(rig)).await;
    match result {
        Ok(proof) => {
            assert!(status.success());
            std::fs::write(evidence, serde_json::to_vec_pretty(&proof).unwrap()).unwrap();
        }
        Err(error) => std::panic::resume_unwind(error),
    }
}

async fn native_outcome(s: &Setup, id: OperationId, expected: OperationOutcome) {
    tokio::time::timeout(std::time::Duration::from_secs(120), async {
        loop {
            let view = s
                .fixture
                .persistence
                .read_operation(
                    s.fixture.target.actor.user_id(),
                    s.scope.company.as_uuid(),
                    id,
                )
                .await
                .unwrap();
            if let Some(outcome) = view.operation.outcome() {
                assert_eq!(outcome, expected);
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
#[ignore = "explicitly authorized disposable native provider acceptance"]
async fn cloudroom_e2e_native_codex_packaged_plugin_pairing_workflow_ui_restart_followup_stop() {
    Box::pin(native_case(LocalProvider::Codex)).await;
}
#[tokio::test]
#[ignore = "explicitly authorized disposable native provider acceptance"]
async fn cloudroom_e2e_native_claude_packaged_plugin_pairing_workflow_ui_restart_followup_stop() {
    Box::pin(native_case(LocalProvider::ClaudeCode)).await;
}
#[tokio::test]
#[ignore = "explicitly authorized disposable native provider acceptance"]
async fn cloudroom_e2e_native_opencode_packaged_plugin_pairing_workflow_ui_restart_followup_stop() {
    Box::pin(native_case(LocalProvider::OpenCode)).await;
}
