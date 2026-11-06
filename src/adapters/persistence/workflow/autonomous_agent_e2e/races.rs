use super::*;

struct Winner {
    revision: PublishedAgentRevision,
    request: PublishAgent,
}

async fn competing_publishers(j: &Journey) -> Winner {
    let left = j.publication_request("competing-left").await;
    let right = j.publication_request("competing-right").await;
    let (a, b) = tokio::join!(
        j.f.persistence.publish_agent(left.clone()),
        j.f.persistence.publish_agent(right.clone()),
    );
    match (a, b) {
        (Ok(revision), Err(AppError::Conflict(_))) => Winner {
            revision,
            request: left,
        },
        (Err(AppError::Conflict(_)), Ok(revision)) => Winner {
            revision,
            request: right,
        },
        (left, right) => panic!(
            "one expected-active publisher wins: {:?}, {:?}",
            left.map(|r| r.id),
            right.map(|r| r.id)
        ),
    }
}

#[tokio::test]
async fn autonomous_agent_e2e_competing_publication_admission_and_lost_commit_replies() {
    let http = scripted_scenario(vec![ScriptedExchange::new(
        |_| Ok(()),
        ScriptedResponse::json(json!({"ok":true})),
    )])
    .await;
    let mcp = mcp_server(Call::Json).await;
    let j = Box::pin(Journey::new(&mcp, &http.base_url)).await;
    let first = competing_publishers(&j).await;
    let workflow = j.workflow().await;
    j.edit(V2).await;
    let publication = j.publication_request("racing-admission").await;
    let identity = j.admission_identity(IdempotencyKey::parse("lost-admission-reply").unwrap());
    let p = &j.f.persistence;
    let left = WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight);
    let right = WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight);
    let (a, b, published) = tokio::join!(
        left.admit(j.admission_request(&workflow, &identity)),
        right.admit(j.admission_request(&workflow, &identity)),
        p.publish_agent(publication),
    );
    let second = published.unwrap();
    let run = match (a.unwrap(), b.unwrap()) {
        (AdmissionResult::Created(a), AdmissionResult::Replayed(b))
        | (AdmissionResult::Replayed(a), AdmissionResult::Created(b)) => {
            assert_eq!(a, b);
            a
        }
        other => panic!("exactly one durable root: {other:?}"),
    };
    let queued = p
        .run_view(RunReadScope {
            company: j.scope.company,
            run,
            viewer: j.scope.actor,
        })
        .await
        .unwrap()
        .unwrap();
    let selection = queued.dependencies.unwrap().agents[0].revision_id;
    assert!(selection == first.revision.id || selection == second.id);
    // Box recovery so this complete worker journey keeps the stock-stack bound.
    let recovered = Box::pin(recover_publication(&j, &first, second.id)).await;
    let service = WorkflowService::new(
        recovered.clone(),
        recovered.clone(),
        recovered.clone(),
        recovered.clone(),
        Preflight,
    );
    assert!(
        matches!(service.admit(j.admission_request(&workflow, &identity)).await.unwrap(), AdmissionResult::Replayed(id) if id == run)
    );
    let mut llm = scripted_llm(turns(1)).await;
    register_scripted_agent_base_url(j.scope.agent.as_uuid(), &llm.base_url);
    j.drive(&mcp, &[run]).await;
    let selected = if selection == first.revision.id {
        &first.revision
    } else {
        &second
    };
    j.assert_selection(run, &workflow, selected).await;
    let observed = llm.observed();
    assert_eq!(observed.len(), 3);
    assert!(observed.iter().all(|r| {
        r["messages"]
            .to_string()
            .contains(&selected.definition.instructions)
    }));
    let counts: Value = sqlx::query_scalar("SELECT jsonb_build_object('runs',(SELECT count(*) FROM workflow_runs WHERE company_id=$1),'attachments',(SELECT count(*) FROM run_dependency_manifests WHERE company_id=$1),'revisions',(SELECT count(*) FROM agent_revisions WHERE company_id=$1 AND agent_id=$2),'publications',(SELECT count(*) FROM agent_publication_requests WHERE company_id=$1 AND agent_id=$2))")
        .bind(j.scope.company.as_uuid()).bind(j.scope.agent.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(
        counts,
        json!({"runs":1,"attachments":1,"revisions":3,"publications":3})
    );
    assert_eq!(mcp.calls(), 1);
    assert_eq!(http.finish().await, Ok(1));
    assert_eq!(llm.finish().await.unwrap(), 3);
}

async fn recover_publication(
    j: &Journey,
    first: &Winner,
    active: crate::entities::agent_capability::AgentRevisionId,
) -> PostgresPersistence {
    let recovered = PostgresPersistence::with_credential_cipher(
        j.f.persistence.pool().clone(),
        CredentialCipher::for_test(),
    );
    j.edit("Unpublished after both durable commits.").await;
    let replay = recovered
        .publish_agent(first.request.clone())
        .await
        .unwrap();
    assert_eq!(replay.id, first.revision.id);
    assert_eq!(
        recovered
            .authoring_state(j.scope)
            .await
            .unwrap()
            .active
            .unwrap()
            .id,
        active
    );
    recovered
}
