use super::*;
use crate::adapters::{
    persistence::credentials::{CredentialCipher, CredentialContext},
    workflow_http::{HttpDestinationPolicy, HttpRuntime},
};
use crate::application::workflow::{
    actions::*, http::*, lease::*, polling::WorkflowStepKind,
};
use crate::domain::entities::correlation::CorrelationId;
use secrecy::ExposeSecret;
use secrecy::SecretString;
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[path = "http_test_support.rs"]
mod support;
use support::*;

#[path = "http_authority_tests.rs"]
mod authority_tests;
use super::action_outcome_support::*;

#[tokio::test]
async fn workflow_http_real_publication_admission_receipt_restart_and_competitors() {
    let (origin, server) =
        server("HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\n{\"ok\":true}").await;
    let setup = Box::pin(setup(&origin, Entrance::Http)).await;
    let runtime = runtime(&setup);
    let (a, b) = tokio::join!(
        runtime.run(&setup.kind, &setup.claim),
        runtime.run(&setup.kind, &setup.claim)
    );
    assert!(a.is_ok() || b.is_ok());
    let request = server.await.unwrap();
    assert!(request.contains("fixture-http-secret"));
    let saved = HttpRuntime::new(
        setup.fixture.persistence.clone(),
        HttpDestinationPolicy::default(),
        lease(),
    )
    .run(&setup.kind, &setup.claim)
    .await
    .unwrap();
    assert_eq!(
        saved["body"]["ok"], true,
        "restart returns committed receipt despite prohibited endpoint"
    );
    assert_eq!(
        action_counts(&setup.fixture).await,
        ActionCounts {
            dispatches: 1,
            entries: 1,
            receipts: 1,
        }
    );
}

#[tokio::test]
async fn workflow_http_registered_tool_and_direct_model_call_share_receipt() {
    let (origin, server) =
        server("HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\n{\"ok\":true}").await;
    let setup = Box::pin(setup(&origin, Entrance::Tool)).await;
    let runtime = runtime(&setup);
    let result = runtime.run(&setup.kind, &setup.claim).await.unwrap();
    server.await.unwrap();
    let call = HttpToolCall {
        connection: setup.connection.id,
        operation: TypeName::parse("registered.request").unwrap(),
        arguments: json!({"method":"POST","path":"/messages","body":{"message":"hello"}}),
        call: ModelToolCallId::parse("model-1").unwrap(),
    };
    let RemoteDispatchObservation::Committed(saved) = runtime
        .call_tool(
            setup.claim.fence,
            call,
            &tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap()
    else {
        panic!("saved receipt")
    };
    assert_eq!(result, saved.result);
    let mappings: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_action_model_calls")
        .fetch_one(setup.fixture.persistence.pool())
        .await
        .unwrap();
    assert_eq!(mappings, 1);
}

#[tokio::test]
async fn workflow_http_remote_failure_remains_unknown_without_second_entry() {
    let (origin, server) = server("HTTP/1.1 500 Error\r\nContent-Length: 0\r\n\r\n").await;
    let setup = Box::pin(setup(&origin, Entrance::Http)).await;
    let runtime = runtime(&setup);
    let failure = runtime.run(&setup.kind, &setup.claim).await.unwrap_err();
    server.await.unwrap();
    assert!(runtime.run(&setup.kind, &setup.claim).await.is_err());
    assert_eq!(
        action_counts(&setup.fixture).await,
        ActionCounts {
            dispatches: 1,
            entries: 1,
            receipts: 0,
        }
    );
    // 04.10-D3: the handler labels Safe; the shared predicate sees the unresolved marker
    // and makes the effective outcome unknown: a settleable reconciliation wait.
    assert_eq!(failure.failure.code().as_str(), "http.unresolved");
    assert_eq!(
        retire(&setup.fixture.persistence, &setup.claim, lease(), failure).await,
        Retired::new(
            "waiting",
            Some("reconciliation"),
            "http.unresolved",
            "retryable",
            "unknown",
            1
        )
    );
}

#[tokio::test]
async fn workflow_http_current_revocation_and_protection_prevent_network() {
    for change in ["connection", "secret", "policy", "approval", "member"] {
        let setup = Box::pin(setup("http://127.0.0.1:1", Entrance::Http)).await;
        let p = &setup.fixture.persistence;
        let company = setup.fixture.target.company;
        let actor = setup.fixture.target.actor;
        match change {
            "connection" => {
                p.set_http_enabled(
                    company,
                    actor,
                    setup.connection.id,
                    setup.connection.revision,
                    false,
                )
                .await
                .unwrap();
            }
            "secret" => {
                p.replace_http_secret(
                    company,
                    actor,
                    setup.connection.id,
                    setup.connection.revision,
                    None,
                )
                .await
                .unwrap();
            }
            "policy" => {
                p.revoke_http_operation(
                    company,
                    actor,
                    setup.connection.id,
                    setup.connection.revision,
                    TypeName::parse("http.request").unwrap(),
                )
                .await
                .unwrap();
            }
            "approval" => {
                let mut op = operation(company, "http.request");
                op.approval_required = true;
                p.register_http_operation(
                    company,
                    actor,
                    setup.connection.id,
                    setup.connection.revision,
                    op,
                )
                .await
                .unwrap();
            }
            _ => {
                sqlx::query("DELETE FROM company_members WHERE company_id=$1 AND user_id=$2")
                    .bind(company.as_uuid())
                    .bind(actor.user_id())
                    .execute(p.pool())
                    .await
                    .unwrap();
            }
        }
        let failure = runtime(&setup)
            .run(&setup.kind, &setup.claim)
            .await
            .unwrap_err();
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_action_dispatches")
            .fetch_one(p.pool())
            .await
            .unwrap();
        assert_eq!(count, 0, "{change}");
        // Pre-marker refusals are classified terminal failures, never waits (04.10-F008);
        // a disabled connection or removed secret is refused by preparation (F012).
        let code = if change == "approval" {
            "http.approval_required"
        } else {
            "http.rejected"
        };
        assert_eq!(
            retire(p, &setup.claim, lease(), failure).await,
            Retired::new("failed", None, code, "terminal", "safe", 0),
            "{change}"
        );
        let attempts: i64 =
            sqlx::query_scalar("SELECT count(*) FROM task_attempts WHERE task_id=$1")
                .bind(setup.claim.fence.scope.job.0)
                .fetch_one(p.pool())
                .await
                .unwrap();
        assert_eq!(attempts, 1, "{change}: no retry loop");
    }
}

#[tokio::test]
async fn workflow_http_crash_before_marker_retries_and_after_marker_reconciles() {
    // No marker: the classifier default is Safe, so lease loss retries instead of parking.
    let unmarked = Box::pin(setup("http://127.0.0.1:1", Entrance::Http)).await;
    let p = &unmarked.fixture.persistence;
    release(p, &unmarked.claim, lease(), LeaseReleaseCause::LeaseLost).await;
    assert_eq!(
        retired(p, &unmarked.claim).await,
        Retired::new(
            "running",
            None,
            "workflow.interrupted",
            "retryable",
            "safe",
            0
        )
    );
    let status: String = sqlx::query_scalar("SELECT status FROM background_tasks WHERE id=$1")
        .bind(unmarked.claim.fence.scope.job.0)
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(status, "pending");
    // A marker without a receipt: the same lease loss is an unknown effect (one fact).
    let (origin, server) = server("HTTP/1.1 500 Error\r\nContent-Length: 0\r\n\r\n").await;
    let marked = Box::pin(setup(&origin, Entrance::Http)).await;
    let p = &marked.fixture.persistence;
    assert!(
        runtime(&marked)
            .run(&marked.kind, &marked.claim)
            .await
            .is_err()
    );
    server.await.unwrap();
    release(p, &marked.claim, lease(), LeaseReleaseCause::LeaseLost).await;
    assert_eq!(
        retired(p, &marked.claim).await,
        Retired::new(
            "waiting",
            Some("reconciliation"),
            "workflow.interrupted",
            "retryable",
            "unknown",
            1
        )
    );
}

#[tokio::test]
async fn workflow_http_secret_is_encrypted_scoped_and_revision_competitors_have_one_winner() {
    let setup = Box::pin(setup("http://127.0.0.1:1", Entrance::Http)).await;
    let p = &setup.fixture.persistence;
    let company = setup.fixture.target.company;
    let actor = setup.fixture.target.actor;
    let envelope: String = sqlx::query_scalar(
        "SELECT envelope FROM workflow_http_credentials WHERE company_id=$1 AND connection_id=$2",
    )
    .bind(company.as_uuid())
    .bind(setup.connection.id)
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert!(!envelope.contains("fixture-http-secret"));
    assert_eq!(
        CredentialCipher::for_test()
            .open_envelope(
                &CredentialContext::workflow_http_credential(
                    company.as_uuid(),
                    setup.connection.id
                ),
                &envelope
            )
            .unwrap()
            .expose_secret(),
        "fixture-http-secret"
    );
    assert!(
        CredentialCipher::for_test()
            .open_envelope(
                &CredentialContext::workflow_http_credential(company.as_uuid(), Uuid::new_v4()),
                &envelope
            )
            .is_err()
    );
    let (a, b) = tokio::join!(
        p.set_http_enabled(
            company,
            actor,
            setup.connection.id,
            setup.connection.revision,
            false
        ),
        p.set_http_enabled(
            company,
            actor,
            setup.connection.id,
            setup.connection.revision,
            true
        )
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert!(
        p.set_http_enabled(
            CompanyId::new(Uuid::new_v4()),
            actor,
            setup.connection.id,
            setup.connection.revision,
            false
        )
        .await
        .is_err()
    );
    let mut tx = p.pool().begin().await.unwrap();
    assert!(
        sqlx::query("UPDATE workflow_http_connections SET origin='https://evil.test' WHERE id=$1")
            .bind(setup.connection.id)
            .execute(&mut *tx)
            .await
            .is_err()
    );
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn workflow_http_foreign_credential_fk_and_nonadmin_authority_fail_closed() {
    let setup = Box::pin(setup("http://127.0.0.1:1", Entrance::Http)).await;
    let p = &setup.fixture.persistence;
    let foreign = Fixture::in_database(setup.fixture._db.clone(), "foreign-http").await;
    let connection = foreign
        .persistence
        .create_http(
            foreign.target.company,
            foreign.target.actor,
            HttpConnectionWrite {
                origin: HttpOrigin::parse("https://example.test").unwrap(),
                bearer: true,
            },
        )
        .await
        .unwrap();
    let mut tx = p.pool().begin().await.unwrap();
    let error=sqlx::query("INSERT INTO workflow_http_credentials(company_id,connection_id,envelope) VALUES($1,$2,'enc:v2:foreign')").bind(setup.fixture.target.company.as_uuid()).bind(connection.id).execute(&mut *tx).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23503")
    );
    tx.rollback().await.unwrap();
    assert!(
        p.create_http(
            setup.fixture.target.company,
            foreign.target.actor,
            HttpConnectionWrite {
                origin: HttpOrigin::parse("https://example.test").unwrap(),
                bearer: false
            }
        )
        .await
        .is_err()
    );
    sqlx::query("UPDATE companies SET user_id=$2 WHERE id=$1")
        .bind(setup.fixture.target.company.as_uuid())
        .bind(foreign.target.actor.user_id())
        .execute(p.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE company_members SET role='member' WHERE company_id=$1 AND user_id=$2")
        .bind(setup.fixture.target.company.as_uuid())
        .bind(setup.fixture.target.actor.user_id())
        .execute(p.pool())
        .await
        .unwrap();
    assert!(
        p.set_http_enabled(
            setup.fixture.target.company,
            setup.fixture.target.actor,
            setup.connection.id,
            setup.connection.revision,
            false
        )
        .await
        .is_err()
    );
    assert!(
        runtime(&setup)
            .run(&setup.kind, &setup.claim)
            .await
            .is_err()
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_action_dispatches")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

struct ForgedDirectory(ToolSnapshot);
#[async_trait]
impl PublicationDirectory for ForgedDirectory {
    async fn capture(
        &self,
        _: WorkflowActor,
        _: &CompanyWorkflowDraft,
    ) -> AppResult<PublicationDependencies> {
        Ok(PublicationDependencies {
            snapshots: DependencySnapshots {
                tools: vec![self.0.clone()],
                ..Default::default()
            },
            children: vec![],
        })
    }
}
#[tokio::test]
async fn workflow_http_publication_rejects_forged_policy_and_registration_revocation() {
    let setup = Box::pin(setup("http://127.0.0.1:1", Entrance::Http)).await;
    let fixture = &setup.fixture;
    let mut draft = fixture.save(Some(DraftRevision::new(1).unwrap()));
    draft.content.source = registry::example("http.request").unwrap().source;
    fixture.service().save(draft).await.unwrap();
    let mut tool = operation(fixture.target.company, "http.request").tool;
    tool.contract.connection = Some(ResourceName::parse("service").unwrap());
    tool.policy.capability = TypeName::parse("forged.capability").unwrap();
    let definitions = DefinitionService::new(
        Preflight,
        fixture.persistence.clone(),
        WorkflowSourceDecoder,
        ForgedDirectory(tool),
    );
    let mut request = fixture.publish();
    request.expected = DraftRevision::new(2).unwrap();
    request.key = IdempotencyKey::parse("forged").unwrap();
    assert!(definitions.publish(request).await.is_err());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_versions")
        .fetch_one(fixture.persistence.pool())
        .await
        .unwrap();
    assert_eq!(count, 1);
    fixture
        .persistence
        .revoke_http_operation(
            fixture.target.company,
            fixture.target.actor,
            setup.connection.id,
            setup.connection.revision,
            TypeName::parse("http.request").unwrap(),
        )
        .await
        .unwrap();
    let directory = http::HttpPublicationDirectory {
        persistence: fixture.persistence.clone(),
        selections: vec![http::HttpPublicationSelection {
            connection: setup.connection.id,
            slot: Some(ResourceName::parse("service").unwrap()),
            operation: TypeName::parse("http.request").unwrap(),
        }],
    };
    let draft = fixture
        .persistence
        .draft(fixture.target.company, fixture.target.workflow)
        .await
        .unwrap()
        .unwrap();
    assert!(
        directory
            .capture(fixture.target.actor, &draft.draft)
            .await
            .is_err()
    );
}
