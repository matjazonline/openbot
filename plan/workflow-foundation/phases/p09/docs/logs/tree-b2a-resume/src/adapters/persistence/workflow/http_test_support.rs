use super::*;

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
pub(super) struct ActionCounts {
    pub(super) dispatches: i64,
    pub(super) entries: i64,
    pub(super) receipts: i64,
}

pub(super) async fn action_counts(fixture: &Fixture) -> ActionCounts {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM workflow_action_dispatches) AS dispatches,
                (SELECT count(*) FROM workflow_action_remote_entries) AS entries,
                (SELECT count(*) FROM workflow_action_receipts) AS receipts",
    )
    .fetch_one(fixture.persistence.pool())
    .await
    .unwrap()
}

pub(super) fn operation(company: CompanyId, name: &str) -> HttpOperation {
    HttpOperation {
        tool: ToolSnapshot {
            workflow: None,
            company_id: company,
            contract: registry::ToolContract {
                name: TypeName::parse(name).unwrap(),
                connection: None,
                input_schema: json!({"type":"object","properties":{"method":{"type":"string"},"path":{"type":"string"},"headers":{"type":"object"},"body":true},"required":["method","path"],"additionalProperties":false}),
                output_schema: json!(true),
            },
            policy: ApprovedActionPolicy {
                capability: TypeName::parse("http.request").unwrap(),
                policy_revision: 1,
                effect: ActionEffect::Write,
                recovery: ActionRecovery::Reconcile,
            },
        },
        approval_required: false,
    }
}
pub(super) fn lease() -> LeasePolicy {
    LeasePolicy::new(Duration::from_secs(30)).unwrap()
}
pub(super) struct Setup {
    pub(super) version: VersionId,
    pub(super) fixture: Fixture,
    pub(super) connection: HttpConnection,
    pub(super) claim: ClaimedWorkflow,
    pub(super) kind: WorkflowStepKind,
}
#[derive(Clone, Copy)]
pub(super) enum Entrance {
    Http,
    Tool,
}
pub(super) async fn setup(origin: &str, entrance: Entrance) -> Setup {
    let mut fixture = Fixture::new().await;
    fixture.persistence = PostgresPersistence::with_credential_cipher(
        fixture.persistence.pool().clone(),
        CredentialCipher::for_test(),
    );
    // Box the real persistence/lifecycle seams to keep fixture setup on stock stacks.
    let connection = Box::pin(configured(&fixture, origin, entrance)).await;
    let version = Box::pin(published(&fixture, &connection, entrance, None)).await;
    let target = Box::pin(bound(&fixture, &connection, version, entrance)).await;
    let claim = Box::pin(claimed(&fixture, target)).await;
    Setup {
        version,
        fixture,
        connection,
        claim,
        kind: WorkflowStepKind(
            if matches!(entrance, Entrance::Tool) {
                "tool.call"
            } else {
                "http.request"
            }
            .into(),
        ),
    }
}
pub(super) async fn configured(
    fixture: &Fixture,
    origin: &str,
    entrance: Entrance,
) -> HttpConnection {
    let tool = matches!(entrance, Entrance::Tool);
    let p = &fixture.persistence;
    let company = fixture.target.company;
    let actor = fixture.target.actor;
    let connection = p
        .create_http(
            company,
            actor,
            HttpConnectionWrite {
                origin: HttpOrigin::parse(origin).unwrap(),
                bearer: true,
            },
        )
        .await
        .unwrap();
    let connection = p
        .replace_http_secret(
            company,
            actor,
            connection.id,
            connection.revision,
            Some(SecretString::from("fixture-http-secret")),
        )
        .await
        .unwrap();
    let name = if tool {
        "registered.request"
    } else {
        "http.request"
    };
    p.register_http_operation(
        company,
        actor,
        connection.id,
        connection.revision,
        operation(company, name),
    )
    .await
    .unwrap()
}
pub(super) fn published<'a>(
    fixture: &'a Fixture,
    connection: &'a HttpConnection,
    entrance: Entrance,
    approval: Option<Value>,
) -> impl std::future::Future<Output = VersionId> + 'a {
    published_customized(fixture, connection, entrance, approval, |_| {})
}

pub(super) fn published_customized<'a>(
    fixture: &'a Fixture,
    connection: &'a HttpConnection,
    entrance: Entrance,
    approval: Option<Value>,
    customize: impl FnOnce(&mut Value) + 'a,
) -> impl std::future::Future<Output = VersionId> + 'a {
    published_with_operation(fixture, connection, entrance, approval, customize, None)
}

pub(super) async fn published_with_operation(
    fixture: &Fixture,
    connection: &HttpConnection,
    entrance: Entrance,
    approval: Option<Value>,
    customize: impl FnOnce(&mut Value),
    additional: Option<TypeName>,
) -> VersionId {
    let p = &fixture.persistence;
    let tool = matches!(entrance, Entrance::Tool);
    let name = if tool {
        "registered.request"
    } else {
        "http.request"
    };
    let kind = if tool { "tool.call" } else { "http.request" };
    let mut source: Value = serde_json::from_str(&registry::example(kind).unwrap().source).unwrap();
    if tool {
        source["steps"]["start"]["with"] = json!({"tool":{"literal":name},"arguments":{"literal":{"method":"POST","path":"/messages","body":{"message":"hello"}}}});
    }
    if let Some(approval) = approval {
        source["steps"]["start"]["with"]["approval"] = json!({"literal":approval});
    }
    customize(&mut source);
    let mut directory = http::HttpPublicationDirectory {
        persistence: p.clone(),
        selections: vec![http::HttpPublicationSelection {
            connection: connection.id,
            slot: if tool {
                None
            } else {
                Some(ResourceName::parse("service").unwrap())
            },
            operation: TypeName::parse(name).unwrap(),
        }],
    };
    if let Some(operation) = additional {
        directory.selections.push(http::HttpPublicationSelection {
            connection: connection.id,
            slot: None,
            operation,
        });
    }
    let definitions =
        DefinitionService::new(Preflight, p.clone(), WorkflowSourceDecoder, directory);
    let mut draft = fixture.save(None);
    draft.content.source = source.to_string();
    definitions.save(draft).await.unwrap();
    let publication = fixture.publish();
    definitions.publish(publication.clone()).await.unwrap();
    publication.version
}
pub(super) async fn bound(
    fixture: &Fixture,
    connection: &HttpConnection,
    version: VersionId,
    entrance: Entrance,
) -> BindingTarget {
    let p = &fixture.persistence;
    let company = fixture.target.company;
    let actor = fixture.target.actor;
    let tool = matches!(entrance, Entrance::Tool);
    let target = BindingTarget {
        company,
        actor,
        binding: WorkflowBindingId::new(Uuid::new_v4()),
    };
    let bindings = BindingService::new(Preflight, p.clone(), p.clone(), p.clone());
    let state = bindings
        .configure(ConfigureBinding {
            target,
            expected: None,
            association: RelatedAssociation::Company,
            version,
            params: json!({}),
            resources: if tool {
                BTreeMap::new()
            } else {
                BTreeMap::from([(
                    ResourceName::parse("service").unwrap(),
                    RuntimeResourceId::new(connection.id),
                )])
            },
            run_as: None,
            message_trigger: None,
        })
        .await
        .unwrap();
    bindings
        .activate(SetBindingActivity {
            target,
            expected: state.revision,
        })
        .await
        .unwrap();
    target
}
pub(super) async fn claimed(fixture: &Fixture, target: BindingTarget) -> ClaimedWorkflow {
    let p = &fixture.persistence;
    let company = fixture.target.company;
    let actor = fixture.target.actor;
    let admission = WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight)
        .admit(AdmitWorkflowRequest {
            company_id: company,
            actor,
            association: RelatedAssociation::Company,
            trigger: TriggerRef::new(
                company,
                TriggerId::new(Uuid::new_v4()),
                TriggerSource::Manual,
            )
            .unwrap(),
            correlation_id: CorrelationId::new(),
            binding_id: target.binding,
            idempotency_key: IdempotencyKey::parse("http-run").unwrap(),
            input: json!({}),
            rerun_of: None,
            delivery_policy: crate::application::workflow::DeliveryPolicy::Deliver,
            expected_binding_revision: None,
        })
        .await
        .unwrap();
    let run = match admission {
        AdmissionResult::Created(run) => run,
        other => panic!("{other:?}"),
    };
    let (execution,job): (Uuid,Uuid) = sqlx::query_as("SELECT execution.id,job.id FROM workflow_executions AS execution JOIN background_tasks AS job ON job.workflow_execution_id=execution.id WHERE execution.run_id=$1").bind(run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    let scope = crate::application::workflow::activation::ActivationRequest {
        company,
        run,
        execution: ExecutionId::new(execution),
        job: crate::application::workflow::activation::WorkflowJobId(job),
    };
    let claim = p
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap();
    if claim.is_none() {
        let state: Value = sqlx::query_scalar("SELECT jsonb_build_object('run',run.state,'job',job.status,'failures',job.retry_count,'ordinal',job.attempt_ordinal,'continuations',job.continuation_count,'activated',execution.activated_at,'events',(SELECT jsonb_agg(event_kind) FROM workflow_run_events WHERE run_id=run.id)) FROM workflow_runs AS run JOIN workflow_executions AS execution ON execution.run_id=run.id JOIN background_tasks AS job ON job.workflow_execution_id=execution.id WHERE run.id=$1")
            .bind(run.as_uuid()).fetch_one(p.pool()).await.unwrap();
        panic!("claim missing: {state}");
    }
    claim.unwrap()
}

pub(super) async fn server(response: &'static str) -> (String, tokio::task::JoinHandle<String>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = vec![0; 8192];
        let read = socket.read(&mut bytes).await.unwrap();
        socket.write_all(response.as_bytes()).await.unwrap();
        String::from_utf8(bytes[..read].to_vec()).unwrap()
    });
    (origin, task)
}
pub(super) fn runtime(setup: &Setup) -> HttpRuntime {
    HttpRuntime::new(
        setup.fixture.persistence.clone(),
        HttpDestinationPolicy::new([setup.connection.origin.as_str().to_owned()]).unwrap(),
        lease(),
    )
}
