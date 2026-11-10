//! The company memory opt-in form (U1): owner/admin only, cross-company ids concealed, stale
//! revisions refused, and enable refused while the service is unconfigured.
use super::super::super::memory;
use super::*;

const ADMIN_TOKEN: &str =
    "msk_adm_0190f1a2b3c44d5e8f9a0b1c2d3e4f50_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

/// The production composition with memory configured. The service is never contacted here.
fn configured(f: &mut Fixture) {
    let config = crate::infra::memory_config::MemoryServiceConfig {
        base_url: "http://127.0.0.1:9".into(),
        provisioner: crate::infra::memory_config::ProvisionerToken::parse(ADMIN_TOKEN.into())
            .unwrap(),
        request_timeout: std::time::Duration::from_secs(1),
        recall_deadline: std::time::Duration::from_millis(500),
        interactive_in_flight: 1,
        background_in_flight: 1,
    };
    let provisioner =
        Arc::new(crate::adapters::memory_service::MemoryServiceHttp::new(&config).unwrap());
    f.engine = crate::infra::workflow::WorkflowEngine::for_tests(f.persistence.clone())
        .with_memory(
            provisioner.clone(),
            provisioner,
            config.provisioner.credential_id(),
            config.background_in_flight,
        )
        .services();
}

fn form(action: &str, expected_revision: i64) -> axum::Form<memory::MemoryForm> {
    axum::Form(memory::MemoryForm {
        action: action.into(),
        expected_revision,
    })
}

#[tokio::test]
async fn memory_scope_company_settings_form_is_owner_admin_only_and_revision_checked() {
    let mut f = Fixture::new().await.unwrap();
    // Unconfigured: the section says so and enable is refused before any write.
    let html = memory::show(f.ui(f.owner), Path(f.company))
        .await
        .unwrap()
        .0;
    assert!(html.contains("not configured") && !html.contains("<form"));
    let html = memory::command(f.ui(f.owner), Path(f.company), form("enable", 0))
        .await
        .unwrap()
        .0;
    assert!(html.contains("not configured"), "{html}");

    configured(&mut f);
    // A member, a stranger, and the owner against the stranger's company are all denied.
    for (user, company) in [
        (f.member, f.company),
        (f.stranger, f.company),
        (f.owner, f.other),
    ] {
        let denied = memory::show(f.ui(user), Path(company)).await.unwrap().0;
        assert!(denied.contains("require company owner or admin") && !denied.contains("<form"));
        let denied = memory::command(f.ui(user), Path(company), form("enable", 0))
            .await
            .unwrap()
            .0;
        assert!(denied.contains("require company owner or admin"));
    }
    let html = memory::command(f.ui(f.admin), Path(f.company), form("enable", 0))
        .await
        .unwrap()
        .0;
    assert!(
        html.contains("Memory settings saved.")
            && html.contains("Memory is enabled (revision 1")
            && html.contains("Disable memory"),
        "{html}"
    );
    // A stale form is refused with an actionable message, and nothing changes.
    let html = memory::command(f.ui(f.owner), Path(f.company), form("disable", 0))
        .await
        .unwrap()
        .0;
    assert!(html.contains("reload and retry") && html.contains("Memory is enabled"));
    let html = memory::command(f.ui(f.owner), Path(f.company), form("disable", 1))
        .await
        .unwrap()
        .0;
    assert!(html.contains("Memory is disabled (revision 2") && html.contains("Enable memory"));
    // The owner's re-drive (r2 m3): offered only when something failed, owner/admin only,
    // revision-checked, and it changes no revision or epoch.
    let html = memory::command(f.ui(f.owner), Path(f.company), form("enable", 2))
        .await
        .unwrap()
        .0;
    assert!(html.contains("Memory is enabled (revision 3") && !html.contains("Retry failed"));
    sqlx::query(
        "UPDATE memory_lifecycles SET state = 'failed', outcome_code = 'protocol_error', attempts = 8 \
         WHERE company_id = $1",
    )
    .bind(f.company)
    .execute(f.persistence.pool())
    .await
    .unwrap();
    let html = memory::show(f.ui(f.admin), Path(f.company))
        .await
        .unwrap()
        .0;
    assert!(html.contains("Retry failed provisioning"), "{html}");
    let denied = memory::command(f.ui(f.member), Path(f.company), form("retry", 3))
        .await
        .unwrap()
        .0;
    assert!(denied.contains("require company owner or admin"));
    let stale = memory::command(f.ui(f.admin), Path(f.company), form("retry", 2))
        .await
        .unwrap()
        .0;
    assert!(stale.contains("reload and retry") && stale.contains("Retry failed provisioning"));
    let html = memory::command(f.ui(f.admin), Path(f.company), form("retry", 3))
        .await
        .unwrap()
        .0;
    assert!(
        html.contains("Retrying 1 failed")
            && html.contains("revision 3")
            && !html.contains("Retry failed provisioning"),
        "{html}"
    );
    let epoch: i64 =
        sqlx::query_scalar("SELECT disable_epoch FROM memory_tenants WHERE company_id = $1")
            .bind(f.company)
            .fetch_one(f.persistence.pool())
            .await
            .unwrap();
    assert_eq!(epoch, 1, "the re-drive disabled nothing");
    assert!(
        memory::command(f.ui(f.owner), Path(f.company), form("purge", 3))
            .await
            .unwrap()
            .0
            .contains("Unknown memory command")
    );
}

fn capture_form(
    agent: Uuid,
    kind: &str,
    enabled: bool,
    expected_revision: i64,
    reason: &str,
) -> axum::Form<memory::CaptureForm> {
    axum::Form(memory::CaptureForm {
        agent,
        kind: kind.into(),
        enabled,
        allowance_tokens: 4_000,
        max_output_tokens: 500,
        expiry_hours: 12,
        reason: reason.into(),
        expected_revision,
    })
}

/// E8 (U3): the per-agent capture policy form is owner/admin only, needs memory enabled to turn
/// on, is revision-checked, validates its bounds and reason, records who and why, and states
/// both capture reaches on the page; turning thread capture on repeats the cross-channel text.
#[tokio::test]
async fn memory_capture_settings_form_is_owner_admin_only_revision_checked_and_states_its_reach() {
    let mut f = Fixture::new().await.unwrap();
    configured(&mut f);
    let agent = f.agent("Capture agent", "capture-agent").await;
    let capture = |user, company, form| memory::capture(f.ui(user), Path(company), form);
    let html = capture(
        f.admin,
        f.company,
        capture_form(agent, "threads", true, 0, "Pilot"),
    )
    .await
    .unwrap()
    .0;
    assert!(html.contains("Memory is not enabled"), "{html}");
    let enabled = memory::command(f.ui(f.admin), Path(f.company), form("enable", 0))
        .await
        .unwrap()
        .0;
    assert!(enabled.contains("Memory is enabled"));
    let html = memory::show(f.ui(f.owner), Path(f.company))
        .await
        .unwrap()
        .0;
    assert!(
        html.contains("Capture agent")
            && html.contains("Private sessions: capture is off (revision 0)")
            && html.contains("Threads: capture is off (revision 0)")
            && html.contains("lowest-reader level")
            && html.contains("captured at level 0")
            && html
                .contains("scheduled, message-triggered and child runs never reach private memory")
            && html.contains("whatever started the run, customer messages included")
            && html.contains(
                "Child branches, failed or cancelled runs and samples are never captured"
            )
            && html.contains("Capture stays idle while generation is off"),
        "{html}"
    );
    for (user, company) in [
        (f.member, f.company),
        (f.stranger, f.company),
        (f.owner, f.other),
    ] {
        let denied = capture(
            user,
            company,
            capture_form(agent, "threads", true, 0, "Pilot"),
        )
        .await
        .unwrap()
        .0;
        assert!(denied.contains("require company owner or admin") && !denied.contains("<form"));
    }
    let html = capture(
        f.admin,
        f.company,
        capture_form(agent, "threads", true, 0, "Thread pilot"),
    )
    .await
    .unwrap()
    .0;
    assert!(
        html.contains(r#"<p role="status" class="text-sm">Capture settings saved. Thread capture"#)
            && html.contains("any reader cleared to that level, in any channel the agent")
            && html.contains("Threads: capture is on (revision 1)")
            && html.contains("Thread pilot"),
        "{html}"
    );
    // A stale form, a blank reason, an out-of-range bound and an unknown kind change nothing.
    let stale = capture(
        f.owner,
        f.company,
        capture_form(agent, "threads", false, 0, "Stop"),
    )
    .await
    .unwrap()
    .0;
    assert!(stale.contains("reload and retry"), "{stale}");
    let blank = capture(
        f.owner,
        f.company,
        capture_form(agent, "threads", false, 1, "   "),
    )
    .await
    .unwrap()
    .0;
    assert!(blank.contains("Give a reason"), "{blank}");
    let mut over = capture_form(agent, "threads", false, 1, "Stop");
    over.0.max_output_tokens = 9_000;
    let over = capture(f.owner, f.company, over).await.unwrap().0;
    assert!(
        over.contains("The output limit must be 1 to 8192 tokens"),
        "{over}"
    );
    let unknown = capture(
        f.owner,
        f.company,
        capture_form(agent, "email", true, 0, "x"),
    )
    .await
    .unwrap()
    .0;
    assert!(unknown.contains("Unknown capture kind"));
    assert!(unknown.contains("Threads: capture is on (revision 1)"));
    let html = capture(
        f.owner,
        f.company,
        capture_form(agent, "threads", false, 1, "Stop"),
    )
    .await
    .unwrap()
    .0;
    assert!(
        html.contains(r#"<p role="status" class="text-sm">Capture settings saved.</p>"#)
            && html.contains("Threads: capture is off (revision 2)"),
        "{html}"
    );
    let (enabled, by, reason): (bool, Uuid, String) = sqlx::query_as(
        "SELECT enabled, changed_by, reason FROM memory_agent_capture \
         WHERE company_id = $1 AND agent_id = $2 AND kind = 'threads'",
    )
    .bind(f.company)
    .bind(agent)
    .fetch_one(f.persistence.pool())
    .await
    .unwrap();
    assert_eq!((enabled, by, reason.as_str()), (false, f.owner, "Stop"));
}
