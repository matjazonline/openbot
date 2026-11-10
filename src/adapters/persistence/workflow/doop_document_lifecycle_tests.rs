use super::*;
use crate::application::use_cases::mcp::McpUseCases;
use crate::application::workflow::worker::WorkflowHandler;

fn service(d: &Document) -> McpUseCases {
    let p = Arc::new(d.f.persistence.clone());
    McpUseCases::new(
        p.clone(),
        p.clone(),
        p,
        d.client.clone(),
        Arc::new(crate::use_cases::mcp::UnavailableMcpDiscovery),
    )
}

async fn change(d: &Document, action: &str) {
    let p = &d.f.persistence;
    let current = connection_on(p, d.target.company.as_uuid(), d.connection).await;
    let service = service(d);
    let actor = d.target.actor.user_id();
    match action {
        "rotate" => {
            let revision = service
                .replace_token(
                    actor,
                    &current,
                    Some(SecretString::from("rotated-doop-fixture-key")),
                )
                .await
                .unwrap();
            assert!(revision > current.revision);
        }
        "disable" => {
            service
                .update(
                    actor,
                    &current,
                    McpConnectionWrite {
                        slug: current.slug.clone(),
                        endpoint: current.endpoint.clone(),
                        enabled: false,
                        auth: current.auth.clone(),
                        discovered_tools: current.discovered_tools.clone(),
                        tool_grants: current.tool_grants.clone(),
                    },
                )
                .await
                .unwrap();
        }
        "disconnect" => service.delete(actor, &current).await.unwrap(),
        _ => panic!("fixed lifecycle action"),
    }
}

#[tokio::test]
async fn doop_lifecycle_disabled_and_same_slug_replacement_cannot_rebind_a_frozen_document() {
    let d = Box::pin(Document::new(McpErrorPolicy::FailStep)).await;
    let claim = recovery_tests::approved_create(&d).await;
    change(&d, "disable").await;
    let failure = Box::pin(d.engine.handler().execute(&kind(), &claim))
        .await
        .completed_result()
        .unwrap_err();
    assert_eq!(code(&failure), "mcp.rejected");
    change(&d, "disconnect").await;
    let replacement = register(&d.f, &d.server.url).await;
    assert_ne!(replacement.id, d.connection);
    assert_eq!(replacement.slug.as_str(), "doop");
    let failure = Box::pin(d.engine.handler().execute(&kind(), &claim))
        .await
        .completed_result()
        .unwrap_err();
    assert_eq!(code(&failure), "mcp.rejected");
    assert_eq!(d.server.call_names(), vec!["get_profile"]);
    let rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM company_mcp_credentials WHERE company_id=$1 AND connection_id=$2",
    )
    .bind(d.target.company.as_uuid())
    .bind(d.connection)
    .fetch_one(d.f.persistence.pool())
    .await
    .unwrap();
    assert_eq!(rows, 0);
    d.client.shutdown().await.unwrap();
}

#[tokio::test]
async fn doop_lifecycle_rotation_disable_and_disconnect_race_two_workers_before_create_entry() {
    for action in ["rotate", "disable", "disconnect"] {
        let d = Box::pin(Document::new(McpErrorPolicy::FailStep)).await;
        let scope = d.runtime().admit(input()).await.unwrap();
        d.drive(scope, |_, decisions| !decisions.is_empty()).await;
        let decisions = d.runtime().approvals(scope).await;
        assert_eq!(decisions.len(), 1);
        d.runtime().approve(scope, decisions[0]).await;
        d.server.hang_list(true);
        // Document::drive starts two real competing production worker groups.
        let run = d.drive(scope, |view, _| {
            view.detail.summary.waiting_reason.as_deref() == Some("reconciliation")
        });
        let revoke = async {
            tokio::time::timeout(Duration::from_secs(20), d.server.state.arrived.notified())
                .await
                .unwrap();
            change(&d, action).await;
            d.server.hang_list(false);
            d.server.state.release.notify_one();
        };
        let (view, ()) = tokio::join!(run, revoke);
        assert!(
            view.detail.summary.state == "failed"
                || view.detail.summary.waiting_reason.as_deref() == Some("reconciliation")
        );
        assert_eq!(
            d.server.call_names(),
            vec!["get_profile"],
            "current authority must be checked again before create entry: {action}"
        );
        d.client.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn doop_lifecycle_rotation_or_disable_after_accepted_create_requires_manual_reconciliation() {
    for action in ["rotate", "disable"] {
        let d = Box::pin(Document::new(McpErrorPolicy::FailStep)).await;
        let claim = recovery_tests::approved_create(&d).await;
        d.server.tool_mode(tool_name("create_canvas"), Call::Hang);
        {
            let kind = kind();
            let pending = Box::pin(d.engine.handler().execute(&kind, &claim));
            tokio::pin!(pending);
            tokio::select! {
                _ = d.server.state.arrived.notified() => change(&d, action).await,
                result = &mut pending => panic!("create must have entered before changing authority: {result:?}"),
            }
            // Lose the response after remote acceptance, then settle the real worker lease.
        }
        d.server.state.release.notify_one();
        d.server.observed("POST notifications/cancelled").await;
        release(
            &d.f.persistence,
            &claim,
            lease(),
            LeaseReleaseCause::LeaseLost,
        )
        .await;
        let state = retired(&d.f.persistence, &claim).await;
        assert_eq!(
            state,
            Retired::new(
                "waiting",
                Some("reconciliation"),
                "workflow.interrupted",
                "retryable",
                "unknown",
                1
            )
        );
        let failure = Box::pin(d.engine.handler().execute(&kind(), &claim))
            .await
            .completed_result()
            .unwrap_err();
        assert_eq!(code(&failure), "mcp.rejected");
        assert_eq!(retired(&d.f.persistence, &claim).await, state);
        assert_eq!(
            d.server
                .call_names()
                .iter()
                .filter(|name| name.as_str() == "create_canvas")
                .count(),
            1
        );
        assert!(
            !d.server
                .call_names()
                .iter()
                .any(|name| name.starts_with("delete_"))
        );
        d.client.shutdown().await.unwrap();
    }
}
