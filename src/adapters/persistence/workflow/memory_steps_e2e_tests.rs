//! The `memory.load` step against the real memory service (plan 06B.2), run by
//! `scripts/memory-integration.sh`. OpenBot's admission, audience, run ceiling and step handler
//! run unchanged; the namespace is provisioned by the real lifecycle worker and the memories are
//! written by an independent client of the service. `#[ignore]`: it needs that service.
use super::*;
use crate::adapters::persistence::memory::live_tests::{
    Live, call, client, eventually, live, reconcile_with,
};

/// Memory enabled for the fixture's company, its company namespace provisioned on the real
/// service, and ten memories at levels 0–9 written there by an independent client.
async fn live_memory(
    f: &AdmissionFixture,
    scope: ActivationRequest,
    live: &Live,
) -> (PostgresPersistence, Arc<MemoryServiceHttp>) {
    let persistence = PostgresPersistence::with_credential_cipher(
        f.persistence().pool().clone(),
        CredentialCipher::for_test_with_keys(&[(1, [7_u8; 32])], 1),
    );
    persistence
        .enable_memory(
            scope.company,
            f.binding.target.actor,
            0,
            live.provisioner.credential_id(),
        )
        .await
        .unwrap();
    persistence
        .ensure_namespace(scope.company, NamespaceOwner::Company)
        .await
        .unwrap();
    let http = client(live);
    reconcile_with(&persistence, live, &http).await;
    let namespace: Uuid = sqlx::query_scalar(
        "SELECT id FROM memory_namespaces WHERE company_id=$1 AND scope_kind='company' \
         AND state='ready'",
    )
    .bind(scope.company.as_uuid())
    .fetch_one(persistence.pool())
    .await
    .unwrap();
    let tenant = scope.company.as_uuid().hyphenated().to_string();
    let namespace = namespace.hyphenated().to_string();
    let writer = call(
        live,
        "admin/v1/credentials/issue",
        &live.operator,
        json!({"tenant": tenant, "label": "e2e writer", "expires_at": null,
               "grants": [{"namespace": namespace, "read_max": 9, "write_max": 9,
                           "operations": ["save", "search"]}]}),
    )
    .await["token"]
        .as_str()
        .unwrap()
        .to_owned();
    for level in 0..=9_u8 {
        call(
            live,
            "v1/observations/save",
            &writer,
            json!({"idempotency_key": format!("e2e.level.{}.{level}", scope.company.as_uuid().simple()),
                   "expires_at": (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
                   "namespace": namespace, "session_id": null,
                   "context": {"producing_ceiling": level, "consumed": {"kind": "exact", "level": level},
                               "sources": [], "external_sources": [], "expected_access_level": null},
                   "items": [{"kind": "fact", "title": format!("Concise replies {level}"),
                              "body": "The customer prefers concise replies.",
                              "occurred_at": "2026-11-01T00:00:00Z", "expires_at": null}]}),
        )
        .await;
    }
    eventually("all ten memories are searchable", async || {
        let found = call(
            live,
            "v1/observations/search",
            &writer,
            json!({"namespaces": [namespace], "read_ceiling": 9, "query": "concise replies",
                   "limit": 20}),
        )
        .await;
        (found["items"].as_array().unwrap().len() == 10).then_some(())
    })
    .await;
    (persistence, http)
}

/// 06B.2 with the real service: a thread run with readers 6, 8 and 9 is admitted at ceiling 6,
/// and its load returns exactly levels 0–6 of the ten stored. A reader at 3 joining afterwards
/// lowers the read to 0–3 (an audience downgrade). Once the run holds level-4 content, that drop
/// refuses the load before any read.
#[tokio::test]
#[ignore = "needs the live memory service: scripts/memory-integration.sh"]
async fn memory_e2e_thread_readers_6_8_9_load_only_levels_0_to_6_from_the_real_service() {
    use super::super::super::admission_history_tests::Conversation;
    let live = live();
    for case in ["audience", "drop", "refused"] {
        let mut load: Value =
            serde_json::from_str(&registry::example("memory.load").unwrap().source).unwrap();
        load["steps"]["start"]["with"] = json!({"scope":{"literal":{"kind":"company"}},
            "query":{"literal":"concise replies"},"limit":{"literal":20}});
        let f = AdmissionFixture::with_binding(BindingFixture::from_source(load).await).await;
        let c = Conversation::new(&f).await;
        for level in [None, Some(AccessLevel::L8), Some(AccessLevel::L6)] {
            thread_reader(&f, c.channel, c.thread, level).await;
        }
        let command = f.prepare(c.request(&f, case, c.message)).await;
        f.persistence().admit(&command).await.unwrap();
        let job =
            sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id=$1")
                .bind(command.first_execution_id().as_uuid())
                .fetch_one(f.persistence().pool())
                .await
                .unwrap();
        let scope = ActivationRequest {
            company: command.company_id(),
            run: command.proposed_run_id(),
            execution: command.first_execution_id(),
            job: WorkflowJobId(job),
        };
        let (p, http) = live_memory(&f, scope, &live).await;
        if case != "audience" {
            thread_reader(&f, c.channel, c.thread, Some(AccessLevel::L3)).await;
        }
        if case == "refused" {
            raise(p.pool(), scope, AccessLevel::L4).await;
        }
        let claim = p
            .settled()
            .claim_io(scope, worker(), policy())
            .await
            .unwrap()
            .unwrap();
        let runtime = MemoryStepRuntime::new(p.clone(), http);
        let outcome = runtime
            .execute(&WorkflowStepKind("memory.load".into()), &claim)
            .await;
        let consumptions = || async {
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM workflow_memory_consumptions \
                 WHERE company_id=$1 AND execution_id=$2",
            )
            .bind(scope.company.as_uuid())
            .bind(scope.execution.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap()
        };
        if case == "refused" {
            let WorkflowHandlerOutcome::Completed(Err(failure)) = outcome else {
                panic!("the drop refuses the load: {outcome:?}");
            };
            assert_eq!(
                failure.failure.code().as_str(),
                crate::app_error::KNOWLEDGE_AUDIENCE_BELOW_CONTENT
            );
            assert_eq!(consumptions().await, 0, "nothing was read");
            assert_eq!(level(&p, scope).await, 4);
            continue;
        }
        let WorkflowHandlerOutcome::Completed(Ok(loaded)) = outcome else {
            panic!("{case}: memory.load completes: {outcome:?}");
        };
        let expected: u8 = if case == "audience" { 6 } else { 3 };
        let mut levels: Vec<u64> = loaded["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["access_level"].as_u64().unwrap())
            .collect();
        levels.sort_unstable();
        assert_eq!(
            levels,
            (0..=u64::from(expected)).collect::<Vec<_>>(),
            "{case}"
        );
        assert_eq!(loaded["exposed_level"], expected, "{case}");
        assert_eq!(level(&p, scope).await, i16::from(expected), "{case}");
        assert_eq!(consumptions().await, 1, "{case}");
    }
}
