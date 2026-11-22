use super::*;
use crate::entities::transport::PrincipalId;

fn pack(
    namespace: Uuid,
    label: &str,
    count: usize,
) -> (
    crate::application::workflow::memory_recall::RecallNamespace,
    crate::application::memory::MemoryPack,
) {
    use crate::application::memory::{MemoryPack, MemoryPackItem, NamespaceExternalId};
    use crate::application::workflow::memory_recall::RecallNamespace;
    (
        RecallNamespace {
            namespace: NamespaceExternalId(namespace),
            label: label.into(),
            archived: false,
        },
        MemoryPack {
            items: (0..count)
                .map(|_| MemoryPackItem {
                    observation_id: Uuid::new_v4(),
                    kind: "fact".into(),
                    title: "Recall fact".into(),
                    text: "A bounded recalled fact.".into(),
                    occurred_at: chrono::Utc::now(),
                    access_level: 2,
                    revision: 1,
                    content_sha256: "a".repeat(64),
                    sources_sha256: "b".repeat(64),
                })
                .collect(),
            exposed_level: 2,
            pack_digest: "c".repeat(64),
            token_count: count as u32 * 40,
            valid_until: None,
            omitted: 0,
        },
    )
}

#[tokio::test]
async fn memory_scope_tools_automatic_freeze_bounds_consumption_across_ready_namespaces() {
    use crate::application::workflow::agent::{AgentPreparation, AgentPreparationResult};
    use crate::application::workflow::memory_recall::{FetchedRecall, MemoryPreparation};
    let t = Tools::new(AccessLevel::L5).await;
    let root = t.ready(t.root_owner().await).await;
    let personal = t.ready(t.agent_owner().await).await;
    let prepared = t
        .p()
        .prepare_agent_with_recall(
            &t.s.claim,
            lease(),
            MemoryPreparation {
                recall: Some(FetchedRecall::Packs {
                    read_ceiling: AccessLevel::L5,
                    packs: vec![
                        pack(root, "unit:workflow-tests", 40),
                        pack(personal, "personal", 40),
                    ],
                }),
                capture: false,
            },
        )
        .await
        .unwrap();
    let AgentPreparationResult::Ready(launch) = prepared else {
        panic!("freeze is ready")
    };
    let memory = launch.prepared.frozen_memory().unwrap();
    assert_eq!(
        memory.items.len(),
        64,
        "one global bound, not 64 per namespace"
    );
    assert_eq!(memory.namespaces.len(), 2);
    assert_eq!(
        memory
            .items
            .iter()
            .filter(|item| item.namespace.0 == root)
            .count(),
        40
    );
    assert_eq!(
        memory
            .items
            .iter()
            .filter(|item| item.namespace.0 == personal)
            .count(),
        24
    );
    let committed = t.committed().await;
    assert_eq!(committed.consumptions.len(), 1);
    assert_eq!(
        committed.consumptions[0]["items"].as_array().unwrap().len(),
        64
    );
    assert_eq!(
        committed.level, 2,
        "record and rise commit before any provider exposure"
    );
    assert!(
        t.fake.calls().is_empty(),
        "the controlled fetched DTO enters the production freeze"
    );
}

#[tokio::test]
async fn memory_scope_tools_automatic_freeze_refuses_a_foreign_namespace_even_for_an_empty_pack() {
    use crate::application::workflow::agent::AgentPreparation;
    use crate::application::workflow::memory_recall::{FetchedRecall, MemoryPreparation};
    let t = Tools::new(AccessLevel::L5).await;
    t.ready(t.root_owner().await).await;
    let result = t
        .p()
        .prepare_agent_with_recall(
            &t.s.claim,
            lease(),
            MemoryPreparation {
                recall: Some(FetchedRecall::Packs {
                    read_ceiling: AccessLevel::L5,
                    packs: vec![pack(Uuid::new_v4(), "unit:workflow-tests", 0)],
                }),
                capture: false,
            },
        )
        .await;
    assert!(matches!(result,Err(AppError::BadRequest(code)) if code=="memory.scope_unavailable"));
    assert!(t.committed().await.consumptions.is_empty());
    assert!(t.fake.calls().is_empty());
}

#[tokio::test]
async fn memory_scope_tools_preparation_freezes_host_scopes_before_provider_calls() {
    use crate::application::workflow::agent::{AgentPreparation, AgentPreparationResult};

    let t = Tools::new(AccessLevel::L5).await;
    let prepared = t.p().prepare_agent(&t.s.claim, lease()).await.unwrap();
    let AgentPreparationResult::Ready(prepared) = prepared else {
        panic!("fresh own-session preparation must be ready")
    };
    assert!(
        prepared
            .prepared
            .preamble()
            .contains("Memory write scope: personal")
    );
    let spec = prepared.prepared.spec(t.s.claim.fence).unwrap();
    assert_eq!(
        spec.prepared["scope_context"],
        json!({"write_unit":null,"readable_units":["workflow-tests"]})
    );
    let restored = t.p().prepare_agent(&t.s.claim, lease()).await.unwrap();
    let AgentPreparationResult::Ready(restored) = restored else {
        panic!("frozen preparation must restore")
    };
    assert_eq!(
        restored.prepared.spec(t.s.claim.fence).unwrap().prepared,
        spec.prepared
    );
    assert!(
        t.fake.calls().is_empty(),
        "preparation cannot expose service data"
    );
}

#[tokio::test]
async fn memory_scope_tools_empty_read_retains_namespace_authority_and_refusal_cannot_replay_revoked_data()
 {
    let t = Tools::new(AccessLevel::L5).await;
    t.ready(t.root_owner().await).await;
    let owner = t.agent_owner().await;
    let namespace = t.ready(owner).await;
    let call = t
        .prepare(
            MemoryTool::Search,
            json!({"query":"no matching memory","scopes":["personal"]}),
        )
        .await;
    let result = t.call(copy(&call)).await;
    assert!(result["items"].as_array().unwrap().is_empty());
    let namespaces: Vec<Uuid> = sqlx::query_scalar(
        "SELECT namespace_ids FROM workflow_memory_consumptions
        WHERE company_id=$1 AND run_id=$2 AND invocation_id=$3",
    )
    .bind(t.company().as_uuid())
    .bind(t.s.claim.fence.scope.run.as_uuid())
    .bind(call.invocation().unwrap().0)
    .fetch_one(t.p().pool())
    .await
    .unwrap();
    assert_eq!(namespaces, vec![namespace]);
    let mut tx = t.p().pool().begin().await.unwrap();
    crate::adapters::persistence::memory::retire_namespaces_on(
        &mut tx,
        t.company().as_uuid(),
        owner,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let replay = t
        .runtime()
        .call_memory_tool(t.s.claim.fence, copy(&call))
        .await;
    assert!(matches!(replay,Err(AppError::BadRequest(code)) if code=="memory.scope_unavailable"));
    let refusal = t
        .p()
        .refuse_memory_tool(
            t.s.claim.fence,
            &call,
            json!({"error":{"code":"memory.scope_unavailable","message":"unavailable"}}),
        )
        .await;
    assert!(matches!(refusal,Err(AppError::BadRequest(code)) if code=="memory.scope_unavailable"));
    assert_eq!(
        t.invocation_result(&call).await,
        Some(result),
        "history remains immutable while its unauthorized replay refuses"
    );
}

#[tokio::test]
async fn memory_scope_tools_details_reject_a_namespace_outside_the_snapshot_before_service_calls() {
    let t = Tools::new(AccessLevel::L5).await;
    t.ready(t.root_owner().await).await;
    let call = t
        .prepare(
            MemoryTool::Details,
            json!({"items":[{"namespace":Uuid::new_v4(),"id":Uuid::new_v4()}]}),
        )
        .await;
    let result = t.call(call).await;
    assert_eq!(error_code(&result), "memory.scope_unavailable");
    assert_eq!(t.service_calls("observations/fetch"), 0);
    assert!(t.committed().await.consumptions.is_empty());
}

#[tokio::test]
async fn memory_scope_tools_subject_is_frozen_with_the_producer_and_never_selects_the_target_namespace()
 {
    let t = Tools::new(AccessLevel::L5).await;
    let actor = t.actor().await;
    let person_owner = t.person_owner(actor).await;
    let personal = t.ready(person_owner).await;
    let NamespaceOwner::Principal(agent) = t.agent_owner().await else {
        panic!("agent principal")
    };
    // This fixture creates its agent at the storage layer, without publishing a
    // directory address. An invisible same-company principal is not a subject.
    let hidden = t
        .prepare(
            MemoryTool::Save,
            json!({"facts":["Invisible subject"],"scope":"personal","subject":agent.as_uuid()}),
        )
        .await;
    assert_eq!(
        error_code(&t.call(hidden).await),
        "memory.subject_unavailable"
    );
    sqlx::query(
        "INSERT INTO addresses(company_id,slug,target_kind,principal_id,is_primary)
        VALUES($1,'assistant','principal',$2,true)",
    )
    .bind(t.company().as_uuid())
    .bind(agent.as_uuid())
    .execute(t.p().pool())
    .await
    .unwrap();
    let call = t
        .prepare(
            MemoryTool::Save,
            json!({"facts":["A tagged preference"],"scope":"personal","subject":agent.as_uuid()}),
        )
        .await;
    let invocation = call.invocation().unwrap().0;
    let result = t.call(call).await;
    assert_eq!(result["status"], "accepted", "{result}");
    #[derive(sqlx::FromRow)]
    struct Stored {
        namespace_id: Uuid,
        principal_id: Uuid,
        subject_principal_id: Uuid,
        request: Value,
    }
    let saved: Stored = sqlx::query_as(
        "SELECT namespace_id,principal_id,subject_principal_id,request FROM memory_deliveries
        WHERE company_id=$1 AND invocation_id=$2",
    )
    .bind(t.company().as_uuid())
    .bind(invocation)
    .fetch_one(t.p().pool())
    .await
    .unwrap();
    assert_eq!(saved.namespace_id, personal);
    assert_eq!(saved.principal_id, agent.as_uuid());
    assert_eq!(saved.subject_principal_id, agent.as_uuid());
    assert_eq!(
        saved.request["body"]["items"][0]["subject"],
        json!(agent.as_uuid())
    );
    let foreign = crate::application::use_cases::company::CompanyPersistence::create(
        t.p(),
        actor,
        crate::application::use_cases::company::CompanyWrite {
            name: "Foreign".into(),
            slug: format!("subject-{}", Uuid::new_v4().simple()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let foreign_person = crate::adapters::persistence::memory::person_principal_on(
        &mut t.p().pool().acquire().await.unwrap(),
        foreign.id,
        actor,
    )
    .await
    .unwrap();
    for subject in [foreign_person, PrincipalId::new(Uuid::new_v4())] {
        let call = t
            .prepare(
                MemoryTool::Save,
                json!({"facts":["Forbidden subject"],"subject":subject.as_uuid()}),
            )
            .await;
        let result = t.call(call).await;
        assert_eq!(error_code(&result), "memory.subject_unavailable");
    }
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM memory_deliveries WHERE company_id=$1")
            .bind(t.company().as_uuid())
            .fetch_one(t.p().pool())
            .await
            .unwrap();
    assert_eq!(count, 1, "invalid subjects create no intent");
    assert_eq!(
        t.service_calls("observations/save"),
        0,
        "delivery remains the only save effect path"
    );
}
