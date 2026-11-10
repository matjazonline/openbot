//! MCP producer projection precedes workflow pointer/concat evaluation.
use super::*;
use crate::application::workflow::completion::WorkflowCompletion;

async fn publish(fixture: &Fixture, context: Value) -> LifecycleResult<BindingTarget> {
    let p = &fixture.persistence;
    let connection: Uuid =
        sqlx::query_scalar("SELECT id FROM company_mcp_connections WHERE company_id=$1")
            .bind(fixture.target.company.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    let directory = AgentPublicationDirectory {
        persistence: p.clone(),
        inner: mcp::McpPublicationDirectory {
            persistence: p.clone(),
            selections: vec![mcp::McpPublicationSelection {
                connection,
                slot: ResourceName::parse("service").unwrap(),
                tool: TypeName::parse(TOOL).unwrap(),
            }],
        },
    };
    let mut source = source();
    source["steps"].as_object_mut().unwrap().remove("result");
    let example: Value =
        serde_json::from_str(&registry::example("agent.run").unwrap().source).unwrap();
    source["steps"]["answer"] = example["steps"]["start"].clone();
    source["steps"]["answer"]["with"]["context"] = context;
    source["steps"]["lookup"]["routes"]["success"] = json!("answer");
    let definitions =
        DefinitionService::new(Preflight, p.clone(), WorkflowSourceDecoder, directory);
    let mut draft = fixture.save(None);
    draft.content.source = source.to_string();
    definitions.save(draft).await?;
    let publication = fixture.publish();
    definitions.publish(publication.clone()).await?;
    Ok(bound(fixture, connection, publication.version).await)
}

async fn admit(fixture: &Fixture, target: BindingTarget) -> ClaimedWorkflow {
    let p = &fixture.persistence;
    let company = target.company;
    let admission = WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight)
        .admit(AdmitWorkflowRequest {
            history_session: None,
            company_id: company,
            actor: target.actor,
            association: RelatedAssociation::Company,
            trigger: TriggerRef::new(
                company,
                TriggerId::new(Uuid::new_v4()),
                TriggerSource::Manual,
            )
            .unwrap(),
            correlation_id: CorrelationId::new(),
            binding_id: target.binding,
            idempotency_key: IdempotencyKey::parse("binary-context").unwrap(),
            input: json!({"customer_id":"c-42"}),
            rerun_of: None,
            delivery_policy: crate::application::workflow::DeliveryPolicy::Deliver,
            expected_binding_revision: None,
            admission_claim: None,
        })
        .await
        .unwrap();
    let AdmissionResult::Created(run) = admission else {
        panic!("{admission:?}")
    };
    claim(p, job_scope(p, company, run, "lookup").await).await
}

#[tokio::test]
async fn doop_binary_workflow_context_and_removed_pointer_concat_never_leak_or_reissue() {
    for context in [
        json!({"default":{"ref":"/steps/lookup/output","value":{"literal":null}}}),
        json!({"ref":"/steps/lookup/output/content/0/data"}),
        json!({"concat":[{"literal":"bytes: "},{"ref":"/steps/lookup/output/content/0/data"}]}),
        json!({"concat":[{"literal":"bytes: "},{"default":{"ref":"/steps/lookup/output/content/0/data","value":{"literal":"Binary unavailable. Visual inspection was not performed."}}}]}),
    ] {
        context_case(context).await;
    }
}

async fn context_case(context: Value) {
    let must_refuse = context.get("ref").is_some() || context.pointer("/concat/1/ref").is_some();
    let server = mcp_server(Call::Json).await;
    let raw = super::doop_binary::captured(false);
    server.set_result(raw.clone());
    let (fixture, agent) = Box::pin(published_mcp_agent(&server)).await;
    let p = &fixture.persistence;
    let target = Box::pin(publish(&fixture, context)).await;
    if must_refuse {
        assert!(
            format!("{:?}", target.unwrap_err()).contains("reference.optional"),
            "unconditional extraction is rejected at publication before any model or tool call"
        );
        assert_eq!(server.calls(), 0);
        return;
    }
    let target = target.unwrap();
    let claimed = Box::pin(admit(&fixture, target)).await;
    let mcp = Arc::new(McpRuntime::new(
        p.clone(),
        guarded_client(&server.url),
        lease(),
    ));
    let output = mcp.run(&kind(), &claimed).await.completed_result().unwrap();
    assert_eq!(output["_meta"]["binaryContentOmitted"], true);
    assert_eq!(
        mcp.run(&kind(), &claimed).await.completed_result().unwrap(),
        output
    );
    p.complete_io(crate::application::workflow::lease::FencedWorkflowResult {
        fence: claimed.fence,
        output,
        policy: lease(),
    })
    .await
    .unwrap()
    .unwrap();
    let mut llm = scripted_llm(vec![LlmTurn::text("\"no visual review\"")]).await;
    register_scripted_agent_base_url(agent.as_uuid(), &llm.base_url);
    let next = claim(
        p,
        job_scope(p, target.company, claimed.fence.scope.run, "answer").await,
    )
    .await;
    let runtime = mcp_agent_runtime(p, mcp);
    assert!(
        Box::pin(runtime.execute(&WorkflowStepKind("agent.run".into()), &next))
            .await
            .completed_result()
            .is_ok()
    );
    let messages = llm.observed()[0]["messages"].to_string();
    assert!(messages.contains("Visual inspection was not performed"));
    assert!(!messages.contains(raw["content"][0]["data"].as_str().unwrap()));
    assert_eq!(server.calls(), 1);
    assert_eq!(llm.finish().await.unwrap(), 1);
}
