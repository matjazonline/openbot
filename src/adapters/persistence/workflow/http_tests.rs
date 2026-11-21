use super::*;
use crate::adapters::{
    persistence::credentials::{CredentialCipher, CredentialContext},
    workflow_http::{HttpDestinationPolicy, HttpRuntime},
};
use crate::application::workflow::{actions::*, http::*, lease::*, polling::WorkflowStepKind};
use crate::domain::entities::correlation::CorrelationId;
use secrecy::ExposeSecret;
use secrecy::SecretString;
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[path = "http_test_support.rs"]
pub(super) mod support;
use support::*;

#[path = "http_authority_tests.rs"]
mod authority_tests;
#[path = "channel_reply_knowledge_uncertainty_tests.rs"]
mod final_knowledge;
#[path = "metrics_tests.rs"]
mod metrics_tests;
#[path = "reconciliation_fixture.rs"]
pub(crate) mod reconciliation_fixture;
#[path = "http_reconciliation_integration_tests.rs"]
mod reconciliation_integration_tests;
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
    assert!(a.completed_result().is_ok() || b.completed_result().is_ok());
    let request = server.await.unwrap();
    assert!(request.contains("fixture-http-secret"));
    let saved = HttpRuntime::new(
        setup.fixture.persistence.clone(),
        HttpDestinationPolicy::default(),
        lease(),
    )
    .run(&setup.kind, &setup.claim)
    .await
    .completed_result()
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
    let result = runtime
        .run(&setup.kind, &setup.claim)
        .await
        .completed_result()
        .unwrap();
    server.await.unwrap();
    let call = HttpToolCall {
        origin: crate::application::agent::ToolCallOrigin::Workflow,
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
    let failure = runtime
        .run(&setup.kind, &setup.claim)
        .await
        .completed_result()
        .unwrap_err();
    server.await.unwrap();
    assert!(
        runtime
            .run(&setup.kind, &setup.claim)
            .await
            .completed_result()
            .is_err()
    );
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
                crate::adapters::persistence::test_support::revoke_membership_now(
                    p.pool(),
                    company.as_uuid(),
                    actor.user_id(),
                )
                .await;
            }
        }
        let failure = runtime(&setup)
            .run(&setup.kind, &setup.claim)
            .await
            .completed_result()
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
            .completed_result()
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
            None,
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
            None,
            HttpConnectionWrite {
                origin: HttpOrigin::parse("https://example.test").unwrap(),
                bearer: false
            }
        )
        .await
        .is_err()
    );
    crate::adapters::persistence::test_support::transfer_company_ownership(
        p.pool(),
        setup.fixture.target.company.as_uuid(),
        foreign.target.actor.user_id(),
        crate::entities::company_member::CompanyAccessRole::Member,
    )
    .await;
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
            .completed_result()
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

#[path = "action_approval_http_tests.rs"]
mod approval_tests;

#[path = "agent_runtime_tests.rs"]
mod agent_runtime_tests;

#[path = "knowledge_http_tests.rs"]
mod knowledge_http_tests;

#[path = "vault_tests/mod.rs"]
mod vault_tests;

/// Plan07 decision a: registering and revoking an operation is the connection unit's manager's.
#[tokio::test]
async fn unit_resource_authority_http_operation_register_needs_the_connection_unit() {
    let setup = Box::pin(setup("http://127.0.0.1:1", Entrance::Http)).await;
    let p = &setup.fixture.persistence;
    let company = setup.fixture.target.company;
    let billing = p.test_unit(company.as_uuid(), "billing").await;
    let support = p.test_unit(company.as_uuid(), "support").await;
    p.move_http(
        company,
        setup.fixture.target.actor,
        setup.connection.id,
        crate::entities::department::DepartmentId::new(billing),
    )
    .await
    .unwrap();
    let sibling =
        WorkflowActor::authenticated(p.test_unit_lead(company.as_uuid(), support).await).unwrap();
    let lead =
        WorkflowActor::authenticated(p.test_unit_lead(company.as_uuid(), billing).await).unwrap();
    let refused = p
        .register_http_operation(
            company,
            sibling,
            setup.connection.id,
            setup.connection.revision,
            operation(company, "billing.lookup"),
        )
        .await;
    assert!(matches!(refused, Err(AppError::NotFound(_))), "{refused:?}");
    // The move left the revision alone, so the unit's lead registers against it.
    let registered = p
        .register_http_operation(
            company,
            lead,
            setup.connection.id,
            setup.connection.revision,
            operation(company, "billing.lookup"),
        )
        .await
        .unwrap();
    assert_eq!(registered.unit_id, billing);
}

/// Plan07 G1 r2 F2: an unslotted HTTP operation resolves among connections in reach only, so two
/// sibling units can each own it, and neither substitutes for the other.
#[tokio::test]
async fn unit_resource_selection_unslotted_http_resolution_is_judged_within_reach() {
    let mut fixture = Fixture::new().await;
    fixture.persistence = PostgresPersistence::with_credential_cipher(
        fixture.persistence.pool().clone(),
        CredentialCipher::for_test(),
    );
    let billing_connection = Box::pin(configured(
        &fixture,
        "https://billing.example.test",
        Entrance::Http,
    ))
    .await;
    let support_connection = Box::pin(configured(
        &fixture,
        "https://support.example.test",
        Entrance::Http,
    ))
    .await;
    let p = &fixture.persistence;
    let company = fixture.target.company;
    let unit = crate::entities::department::DepartmentId::new;
    let billing = unit(p.test_unit(company.as_uuid(), "billing").await);
    let support = unit(p.test_unit(company.as_uuid(), "support").await);
    for (connection, target) in [
        (&billing_connection, billing),
        (&support_connection, support),
    ] {
        p.move_http(company, fixture.target.actor, connection.id, target)
            .await
            .unwrap();
    }
    let wanted = operation(company, "http.request").tool;
    let mut tx = p.pool().begin().await.unwrap();
    use super::super::http::{ConnectionReach, registered_on};
    assert_eq!(
        registered_on(
            &mut tx,
            company,
            &wanted,
            ConnectionReach::From(Some(billing))
        )
        .await
        .unwrap(),
        billing_connection.id
    );
    assert_eq!(
        registered_on(
            &mut tx,
            company,
            &wanted,
            ConnectionReach::From(Some(support))
        )
        .await
        .unwrap(),
        support_connection.id
    );
    // The root reaches neither and fails closed; company-wide the operation merely exists, so
    // the support check every binding and admission runs first does not fail (B2-F1).
    assert!(matches!(
        registered_on(&mut tx, company, &wanted, ConnectionReach::From(None)).await,
        Err(AppError::NotFound(_))
    ));
    registered_on(&mut tx, company, &wanted, ConnectionReach::Company)
        .await
        .unwrap();
    // With Billing's connection disabled, Billing's runs do not fall back to Support's.
    tx.rollback().await.unwrap();
    let current = p
        .list_http(company, fixture.target.actor)
        .await
        .unwrap()
        .into_iter()
        .find(|connection| connection.id == billing_connection.id)
        .unwrap();
    p.set_http_enabled(
        company,
        fixture.target.actor,
        current.id,
        current.revision,
        false,
    )
    .await
    .unwrap();
    let mut tx = p.pool().begin().await.unwrap();
    assert!(matches!(
        registered_on(
            &mut tx,
            company,
            &wanted,
            ConnectionReach::From(Some(billing))
        )
        .await,
        Err(AppError::NotFound(_))
    ));
}

/// Plan07 01B.1: the workflow editor offers a definition only connections in reach of its
/// managing department; the binding form offers a lead only those in reach of a unit they manage.
#[tokio::test]
async fn unit_resource_selection_offers_follow_definition_and_author_reach() {
    use crate::application::workflow::reads::{AuthoringReads, PublicationReach};
    let mut fixture = Fixture::new().await;
    fixture.persistence = PostgresPersistence::with_credential_cipher(
        fixture.persistence.pool().clone(),
        CredentialCipher::for_test(),
    );
    let p = &fixture.persistence;
    let company = fixture.target.company;
    let root_connection = Box::pin(configured(
        &fixture,
        "https://root.example.test",
        Entrance::Http,
    ))
    .await;
    let billing_connection = Box::pin(configured(
        &fixture,
        "https://billing.example.test",
        Entrance::Http,
    ))
    .await;
    let support_connection = Box::pin(configured(
        &fixture,
        "https://support.example.test",
        Entrance::Http,
    ))
    .await;
    let billing = p.test_unit(company.as_uuid(), "billing").await;
    let support = p.test_unit(company.as_uuid(), "support").await;
    for (connection, target) in [
        (&billing_connection, billing),
        (&support_connection, support),
    ] {
        p.move_http(
            company,
            fixture.target.actor,
            connection.id,
            crate::entities::department::DepartmentId::new(target),
        )
        .await
        .unwrap();
    }
    // The fixture's definition is managed by the root, which reaches only root connections.
    let options = p
        .publication_options(
            company,
            PublicationReach {
                workflow: fixture.target.workflow,
                author: fixture.target.actor.user_id(),
            },
            &[],
            &["http.request".to_owned()],
        )
        .await
        .unwrap();
    assert_eq!(
        options
            .http
            .iter()
            .map(|option| option.connection)
            .collect::<Vec<_>>(),
        vec![root_connection.id]
    );
    let lead = p.test_unit_lead(company.as_uuid(), billing).await;
    let offered = |options: crate::application::workflow::reads::BindingOptions| {
        let mut ids: Vec<Uuid> = options.http.into_iter().map(|choice| choice.id).collect();
        ids.sort();
        ids
    };
    let mut expected = vec![root_connection.id, billing_connection.id];
    expected.sort();
    assert_eq!(
        offered(p.binding_options(company, lead).await.unwrap()),
        expected
    );
    assert_eq!(
        offered(
            p.binding_options(company, fixture.target.actor.user_id())
                .await
                .unwrap()
        )
        .len(),
        3,
        "a root manager is offered every unit's connections"
    );
}

/// Plan07 01B.2: publishing a workflow refuses an HTTP selection outside its managing
/// department's reach, naming the connection, operation and the step using it, and writes no
/// version.
#[tokio::test]
async fn unit_resource_selection_publication_workflow_refuses_an_http_selection_out_of_reach() {
    let mut fixture = Fixture::new().await;
    fixture.persistence = PostgresPersistence::with_credential_cipher(
        fixture.persistence.pool().clone(),
        CredentialCipher::for_test(),
    );
    let connection = Box::pin(configured(
        &fixture,
        "https://billing.example.test",
        Entrance::Http,
    ))
    .await;
    let p = &fixture.persistence;
    let company = fixture.target.company;
    let definitions = DefinitionService::new(
        Preflight,
        p.clone(),
        WorkflowSourceDecoder,
        http::HttpPublicationDirectory {
            persistence: p.clone(),
            selections: vec![http::HttpPublicationSelection {
                connection: connection.id,
                slot: Some(ResourceName::parse("service").unwrap()),
                operation: TypeName::parse("http.request").unwrap(),
            }],
        },
    );
    let mut draft = fixture.save(None);
    draft.content.source = registry::example("http.request").unwrap().source.clone();
    definitions.save(draft).await.unwrap();
    let billing = p.test_unit(company.as_uuid(), "billing").await;
    p.move_http(
        company,
        fixture.target.actor,
        connection.id,
        crate::entities::department::DepartmentId::new(billing),
    )
    .await
    .unwrap();
    let refused = definitions.publish(fixture.publish()).await;
    assert!(
        refused.as_ref().err().is_some_and(|error| matches!(error,
            crate::application::workflow::lifecycle::LifecycleError::Application(
                AppError::ResourceOutsideUnit(refusal))
            if refusal.code() == "publication.resource_outside_unit"
                && refusal.resource == connection.id
                && refusal.name.contains("http.request")
                && refusal.name.ends_with("used by step start"))),
        "{:?}",
        refused.err()
    );
    let versions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_versions WHERE company_id=$1")
            .bind(company.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(versions, 0, "a refused publication writes no version");
}

/// Plan07 decision d: Agent Builder tool names resolve among connections in reach of the agent's
/// placement only (the root's when none).
#[tokio::test]
async fn unit_resource_selection_agent_builder_tool_names_follow_the_placement_reach() {
    let mut fixture = Fixture::new().await;
    fixture.persistence = PostgresPersistence::with_credential_cipher(
        fixture.persistence.pool().clone(),
        CredentialCipher::for_test(),
    );
    let connection = Box::pin(configured(
        &fixture,
        "https://billing.example.test",
        Entrance::Tool,
    ))
    .await;
    let p = &fixture.persistence;
    let company = fixture.target.company;
    let billing = p.test_unit(company.as_uuid(), "billing").await;
    let support = p.test_unit(company.as_uuid(), "support").await;
    p.move_http(
        company,
        fixture.target.actor,
        connection.id,
        crate::entities::department::DepartmentId::new(billing),
    )
    .await
    .unwrap();
    let names = [TypeName::parse("registered.request").unwrap()];
    let mut db = p.pool().acquire().await.unwrap();
    use crate::adapters::persistence::agent_binding_selection::resolve_bindings_in_reach_on;
    for placement in [None, Some(support)] {
        assert!(
            resolve_bindings_in_reach_on(&mut db, company.as_uuid(), None, &names, placement)
                .await
                .is_err(),
            "{placement:?}"
        );
    }
    let resolved =
        resolve_bindings_in_reach_on(&mut db, company.as_uuid(), None, &names, Some(billing))
            .await
            .unwrap();
    assert_eq!(resolved.len(), 1);
}

fn desk_association(desk: Uuid) -> RelatedAssociation {
    RelatedAssociation::Channel(crate::application::workflow::RelatedChannelId::new(desk))
}

async fn unit_desk(fixture: &Fixture, unit: Uuid) -> Uuid {
    crate::adapters::persistence::test_support::create_channel(
        &fixture.persistence,
        fixture.target.company.as_uuid(),
        crate::application::ports::channel::ChannelWrite {
            name: "Unit desk".into(),
            slug: format!("desk-{}", Uuid::new_v4().simple()),
            department_id: Some(crate::entities::department::DepartmentId::new(unit)),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id
}

fn configure(
    target: BindingTarget,
    association: RelatedAssociation,
    version: VersionId,
    slot: Option<Uuid>,
) -> ConfigureBinding {
    ConfigureBinding {
        escalation: None,
        target,
        expected: None,
        association,
        version,
        params: json!({}),
        resources: slot
            .map(|id| {
                BTreeMap::from([(
                    ResourceName::parse("service").unwrap(),
                    RuntimeResourceId::new(id),
                )])
            })
            .unwrap_or_default(),
        run_as: None,
        message_trigger: None,
    }
}

/// Plan07 G1 r2 F2 (B2-F1, F4): on a Billing desk an unslotted operation that Support also owns
/// routes to Billing's connection: the binding saves, the run is admitted and an identical
/// request replays it, and the action calls Billing's origin. A company-wide binding (the
/// root's reach) reaches neither, and with Billing's connection disabled after admission the
/// action does not fall back to Support's.
#[tokio::test]
async fn unit_resource_selection_desk_routes_an_unslotted_operation_within_its_reach() {
    let (origin, server) =
        server("HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\n{\"ok\":true}").await;
    let mut fixture = Fixture::new().await;
    fixture.persistence = PostgresPersistence::with_credential_cipher(
        fixture.persistence.pool().clone(),
        CredentialCipher::for_test(),
    );
    let billing_connection = Box::pin(configured(&fixture, &origin, Entrance::Tool)).await;
    let version = Box::pin(published(
        &fixture,
        &billing_connection,
        Entrance::Tool,
        None,
    ))
    .await;
    let support_connection =
        Box::pin(configured(&fixture, "http://127.0.0.1:9", Entrance::Tool)).await;
    let p = fixture.persistence.clone();
    let company = fixture.target.company;
    let actor = fixture.target.actor;
    let billing = p.test_unit(company.as_uuid(), "billing").await;
    let support = p.test_unit(company.as_uuid(), "support").await;
    for (connection, unit) in [
        (&billing_connection, billing),
        (&support_connection, support),
    ] {
        p.move_http(
            company,
            actor,
            connection.id,
            crate::entities::department::DepartmentId::new(unit),
        )
        .await
        .unwrap();
    }
    let bindings = BindingService::new(Preflight, p.clone(), p.clone(), p.clone());
    let company_wide = BindingTarget {
        company,
        actor,
        binding: WorkflowBindingId::new(Uuid::new_v4()),
    };
    assert!(
        bindings
            .configure(configure(
                company_wide,
                RelatedAssociation::Company,
                version,
                None
            ))
            .await
            .is_err(),
        "the root reaches neither connection"
    );
    let association = desk_association(unit_desk(&fixture, billing).await);
    let target = Box::pin(bound(
        &fixture,
        &billing_connection,
        version,
        Entrance::Tool,
        association,
    ))
    .await;
    let service = WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight);
    let trigger = TriggerId::new(Uuid::new_v4());
    let correlation = CorrelationId::new();
    let request = || AdmitWorkflowRequest {
        history_session: None,
        company_id: company,
        actor,
        association,
        trigger: TriggerRef::new(company, trigger, TriggerSource::Manual).unwrap(),
        correlation_id: correlation,
        binding_id: target.binding,
        idempotency_key: IdempotencyKey::parse("desk-replay").unwrap(),
        input: json!({}),
        rerun_of: None,
        delivery_policy: crate::application::workflow::DeliveryPolicy::Deliver,
        expected_binding_revision: None,
        admission_claim: None,
    };
    let AdmissionResult::Created(first) = service.admit(request()).await.unwrap() else {
        panic!("admitted on the Billing desk");
    };
    assert_eq!(
        service.admit(request()).await.unwrap(),
        AdmissionResult::Replayed(first)
    );
    let claim = Box::pin(claimed_with_key(&fixture, target, association, "desk-run")).await;
    let second = Box::pin(claimed_with_key(
        &fixture,
        target,
        association,
        "desk-run-2",
    ))
    .await;
    let setup = Setup {
        version,
        fixture,
        connection: billing_connection,
        claim,
        kind: WorkflowStepKind("tool.call".into()),
    };
    runtime(&setup)
        .run(&setup.kind, &setup.claim)
        .await
        .completed_result()
        .unwrap();
    assert!(server.await.unwrap().contains("fixture-http-secret"));
    let current = p
        .list_http(company, actor)
        .await
        .unwrap()
        .into_iter()
        .find(|connection| connection.id == setup.connection.id)
        .unwrap();
    p.set_http_enabled(company, actor, current.id, current.revision, false)
        .await
        .unwrap();
    assert!(
        runtime(&setup)
            .run(&setup.kind, &second)
            .await
            .completed_result()
            .is_err(),
        "no fallback to Support's identical operation"
    );
}

/// Plan07 01B.3/D8 and decision f (B2-F4, F5, F9): a slot connection owned by Billing is usable
/// on a Billing desk by a Billing lead, who does not manage the root, through binding save,
/// admission and the action; a root-owned connection is shared with that desk too; the Billing
/// connection is refused on a Support desk with `binding.resource_outside_unit`, and once moved
/// up to the root it is selectable there.
#[tokio::test]
async fn unit_resource_selection_desk_slot_connection_follows_desk_reach_and_is_shared_below() {
    let (origin, server) =
        server("HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\n{\"ok\":true}").await;
    let mut fixture = Fixture::new().await;
    fixture.persistence = PostgresPersistence::with_credential_cipher(
        fixture.persistence.pool().clone(),
        CredentialCipher::for_test(),
    );
    let connection = Box::pin(configured(&fixture, &origin, Entrance::Http)).await;
    let rooted = Box::pin(configured(&fixture, "http://127.0.0.1:9", Entrance::Http)).await;
    let version = Box::pin(published(&fixture, &connection, Entrance::Http, None)).await;
    let p = fixture.persistence.clone();
    let company = fixture.target.company;
    let owner = fixture.target.actor;
    let billing = p.test_unit(company.as_uuid(), "billing").await;
    let support = p.test_unit(company.as_uuid(), "support").await;
    p.move_http(
        company,
        owner,
        connection.id,
        crate::entities::department::DepartmentId::new(billing),
    )
    .await
    .unwrap();
    let billing_channel = unit_desk(&fixture, billing).await;
    let billing_desk = desk_association(billing_channel);
    let support_desk = desk_association(unit_desk(&fixture, support).await);
    let bindings = BindingService::new(Preflight, p.clone(), p.clone(), p.clone());
    let lead =
        WorkflowActor::authenticated(p.test_unit_lead(company.as_uuid(), billing).await).unwrap();
    // The lead sees the desk, as any run actor must (unchanged by Plan07).
    sqlx::query("INSERT INTO channel_principal_grants(company_id,channel_id,principal_id,capability,provenance) SELECT $1,$2,id,'view','department_membership' FROM principals WHERE company_id=$1 AND user_id=$3")
        .bind(company.as_uuid()).bind(billing_channel).bind(lead.user_id()).execute(p.pool()).await.unwrap();
    let target = |actor| BindingTarget {
        company,
        actor,
        binding: WorkflowBindingId::new(Uuid::new_v4()),
    };
    // Moved up is shared: the lead may bind the root's connection on a Billing desk.
    bindings
        .configure(configure(
            target(lead),
            billing_desk,
            version,
            Some(rooted.id),
        ))
        .await
        .unwrap();
    let led = target(lead);
    let state = bindings
        .configure(configure(led, billing_desk, version, Some(connection.id)))
        .await
        .unwrap();
    bindings
        .activate(SetBindingActivity {
            target: led,
            expected: state.revision,
        })
        .await
        .unwrap();
    // The lead admits the run, as its actor.
    fixture.target.actor = lead;
    let claim = Box::pin(claimed_with_key(&fixture, led, billing_desk, "lead-run")).await;
    fixture.target.actor = owner;
    let refused = bindings
        .configure(configure(
            target(owner),
            support_desk,
            version,
            Some(connection.id),
        ))
        .await;
    assert!(
        refused.as_ref().err().is_some_and(|error| matches!(error,
            crate::application::workflow::lifecycle::LifecycleError::Application(
                AppError::ResourceOutsideUnit(refusal))
            if refusal.code() == "binding.resource_outside_unit"
                && refusal.resource == connection.id)),
        "{:?}",
        refused.err()
    );
    p.move_http(
        company,
        owner,
        connection.id,
        crate::entities::department::DepartmentId::new(p.test_root_unit(company.as_uuid()).await),
    )
    .await
    .unwrap();
    bindings
        .configure(configure(
            target(owner),
            support_desk,
            version,
            Some(connection.id),
        ))
        .await
        .unwrap();
    // Moved back to Billing, the lead's admitted run uses it at action time.
    p.move_http(
        company,
        owner,
        connection.id,
        crate::entities::department::DepartmentId::new(billing),
    )
    .await
    .unwrap();
    let setup = Setup {
        version,
        fixture,
        connection,
        claim,
        kind: WorkflowStepKind("http.request".into()),
    };
    runtime(&setup)
        .run(&setup.kind, &setup.claim)
        .await
        .completed_result()
        .unwrap();
    assert!(server.await.unwrap().contains("fixture-http-secret"));
}

/// Plan07 D9a (B2-F4): admission checks every reachable bundle against the run's reach, so a
/// parent whose `workflow.call` child uses an unslotted operation is refused, with no run
/// written, once that operation's only connection moves out of the root's reach.
#[tokio::test]
async fn unit_resource_selection_admission_workflow_child_bundle_out_of_reach_is_refused() {
    let mut fixture = Fixture::new().await;
    fixture.persistence = PostgresPersistence::with_credential_cipher(
        fixture.persistence.pool().clone(),
        CredentialCipher::for_test(),
    );
    let connection = Box::pin(configured(&fixture, "http://127.0.0.1:9", Entrance::Tool)).await;
    let p = fixture.persistence.clone();
    let child = Fixture {
        _db: fixture._db.clone(),
        persistence: p.clone(),
        target: DefinitionTarget {
            workflow: WorkflowId::new(Uuid::new_v4()),
            ..fixture.target
        },
    };
    let child_workflow = child.target.workflow.to_string();
    let child_version = Box::pin(published_customized(
        &child,
        &connection,
        Entrance::Tool,
        None,
        move |source| source["workflow_id"] = json!(child_workflow),
    ))
    .await;
    let mut source: Value =
        serde_json::from_str(&registry::example("workflow.call").unwrap().source).unwrap();
    source["steps"]["start"]["child_workflow_id"] = json!(child.target.workflow.to_string());
    source["steps"]["start"]["child_version_id"] = json!(child_version.as_uuid().to_string());
    source["steps"]["start"]["with"]["input"] = json!({"literal":{}});
    source["steps"]["start"]["with"]["params"] = json!({"literal":{}});
    let definitions = DefinitionService::new(
        Preflight,
        p.clone(),
        WorkflowSourceDecoder,
        crate::adapters::persistence::workflow::child_publication::ChildPublicationDirectory {
            persistence: p.clone(),
            inner: Preflight,
        },
    );
    let mut draft = fixture.save(None);
    draft.content.source = source.to_string();
    definitions.save(draft).await.unwrap();
    let mut publication = fixture.publish();
    publication.key = IdempotencyKey::parse("publish-parent").unwrap();
    definitions.publish(publication.clone()).await.unwrap();
    let bindings = BindingService::new(Preflight, p.clone(), p.clone(), p.clone());
    let target = BindingTarget {
        company: fixture.target.company,
        actor: fixture.target.actor,
        binding: WorkflowBindingId::new(Uuid::new_v4()),
    };
    let state = bindings
        .configure(configure(
            target,
            RelatedAssociation::Company,
            publication.version,
            None,
        ))
        .await
        .unwrap();
    bindings
        .activate(SetBindingActivity {
            target,
            expected: state.revision,
        })
        .await
        .unwrap();
    let service = WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight);
    let admit = |key: &'static str| AdmitWorkflowRequest {
        history_session: None,
        company_id: target.company,
        actor: target.actor,
        association: RelatedAssociation::Company,
        trigger: TriggerRef::new(
            target.company,
            TriggerId::new(Uuid::new_v4()),
            TriggerSource::Manual,
        )
        .unwrap(),
        correlation_id: CorrelationId::new(),
        binding_id: target.binding,
        idempotency_key: IdempotencyKey::parse(key).unwrap(),
        input: json!({}),
        rerun_of: None,
        delivery_policy: crate::application::workflow::DeliveryPolicy::Deliver,
        expected_binding_revision: None,
        admission_claim: None,
    };
    assert!(matches!(
        service.admit(admit("child-in-reach")).await.unwrap(),
        AdmissionResult::Created(_)
    ));
    let runs = || async {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM workflow_runs WHERE company_id=$1")
            .bind(target.company.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap()
    };
    let before = runs().await;
    let billing = p.test_unit(target.company.as_uuid(), "billing").await;
    p.move_http(
        target.company,
        target.actor,
        connection.id,
        crate::entities::department::DepartmentId::new(billing),
    )
    .await
    .unwrap();
    assert!(service.admit(admit("child-out-of-reach")).await.is_err());
    assert_eq!(runs().await, before, "refused before any run row");
}
