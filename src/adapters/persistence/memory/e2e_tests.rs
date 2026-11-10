//! OpenBot and the real memory service as separate artifacts (plan 06B.2), run by
//! `scripts/memory-integration.sh`: the service is built from services/memory and serves under its
//! own database role, and OpenBot reaches it only through its URL and the provisioner credential.
//! OpenBot's freezes, delivery and lifecycle workers and client reads run unchanged; the real
//! service enforces every label and ceiling. `#[ignore]`: without that service they cannot run.
use super::*;
use crate::adapters::memory_service::MemoryServiceHttp;
use secrecy::ExposeSecret;

/// Freeze a save from a run with these levels into `scope`.
async fn freeze(
    f: &Fixture,
    scope: SaveScope,
    key: &str,
    levels: LockedRun,
    external: Vec<ExternalSourceRef>,
) -> crate::app_error::AppResult<SaveEnqueue> {
    let facts = vec![format!("E2E-{key}: the renewal is due in March.")];
    let intent = SaveIntent {
        company: f.scope(),
        run: RunId::new(Uuid::new_v4()),
        execution: ExecutionId::new(Uuid::new_v4()),
        invocation: None,
        wait: None,
        scope,
        key,
        facts: &facts,
        external_sources: external,
    };
    let mut tx = f.pool().begin().await.unwrap();
    let enqueued = freeze_save_on(&mut tx, &intent, levels).await?;
    tx.commit().await.unwrap();
    Ok(enqueued)
}

/// A save that must freeze.
async fn frozen(
    f: &Fixture,
    scope: SaveScope,
    key: &str,
    levels: LockedRun,
    external: Vec<ExternalSourceRef>,
) {
    assert!(matches!(
        freeze(f, scope, key, levels, external).await.unwrap(),
        SaveEnqueue::Enqueued { .. }
    ));
}

fn run(ceiling: AccessLevel, content: AccessLevel, exact: bool) -> LockedRun {
    LockedRun {
        ceiling,
        content,
        content_exact: exact,
    }
}

/// Enable memory and provision the company and agent namespaces on the real service.
async fn provisioned(f: &Fixture, live: &Live, http: &Arc<MemoryServiceHttp>) {
    f.persistence
        .enable_memory(
            f.scope(),
            Fixture::actor(f.owner),
            0,
            live.provisioner.credential_id(),
        )
        .await
        .unwrap();
    for owner in [NamespaceOwner::Company, NamespaceOwner::Agent(f.agent)] {
        f.persistence
            .ensure_namespace(f.scope(), owner)
            .await
            .unwrap();
    }
    reconcile_with(&f.persistence, live, http).await;
}

/// The saved observation of a delivered intent, with the label the service stored.
async fn saved(f: &Fixture, key: &str) -> (Uuid, u8) {
    let (state, result) = delivery(f, key).await;
    assert_eq!(state, "delivered", "{key}");
    let item = &result.unwrap()["observations"][0];
    (
        serde_json::from_value(item["observation_id"].clone()).unwrap(),
        item["access_level"].as_u64().unwrap() as u8,
    )
}

/// A credential the operator issues straight from the service (not OpenBot's), reading `ns` up to
/// `read_max`, as an independent client would hold.
async fn reader(live: &Live, f: &Fixture, namespace: Uuid, read_max: u8) -> String {
    let issued = call(
        live,
        "admin/v1/credentials/issue",
        &live.operator,
        json!({"tenant": f.company.hyphenated().to_string(), "label": "e2e reader",
               "expires_at": null,
               "grants": [{"namespace": namespace.hyphenated().to_string(), "read_max": read_max,
                           "write_max": 0, "operations": ["search", "fetch", "timeline", "context"]}]}),
    )
    .await;
    issued["token"].as_str().unwrap().to_owned()
}

/// `text` with every service-minted request id blanked: it differs per request by design.
fn without_request_ids(text: &str) -> String {
    let marker = "\"request_id\":\"";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(marker) {
        let value = at + marker.len();
        out.push_str(&rest[..value]);
        let end = rest[value..]
            .find('"')
            .map_or(rest.len(), |close| value + close);
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// Every place a reader could see an observation, at `ceiling`, through the service's HTTP API
/// and its MCP tools. Returns the surfaces that reveal `observation`:
/// - a search or pack answer that mentions it at all (as an item, a parent, a source or a
///   neighbour);
/// - a fetch or timeline answer that differs from the answer for an id that never existed (once
///   the two ids are swapped), so a hidden memory is indistinguishable from a missing one. Fetch
///   echoes a requested id it cannot serve as `unavailable`, for hidden and missing alike.
async fn visible_anywhere(
    live: &Live,
    token: &str,
    namespace: Uuid,
    ceiling: u8,
    observation: Uuid,
) -> Vec<String> {
    let namespaces = json!([namespace.hyphenated().to_string()]);
    let http = reqwest::Client::new();
    let base = live.url.trim_end_matches('/');
    let answer = async |path: &str, body: Value| -> String {
        let response = http
            .post(format!("{base}/{path}"))
            .bearer_auth(token)
            .header("accept", "application/json, text/event-stream")
            .json(&body)
            .send()
            .await
            .unwrap();
        format!("{} {}", response.status(), response.text().await.unwrap())
    };
    let tool = |name: &str, arguments: Value| {
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
               "params": {"name": name, "arguments": arguments}})
    };
    let id = observation.hyphenated().to_string();
    let missing = Uuid::new_v4().hyphenated().to_string();
    let search = json!({"namespaces": namespaces, "read_ceiling": ceiling,
                        "query": "renewal March", "limit": 50});
    let pack = json!({"namespaces": namespaces, "read_ceiling": ceiling, "query": "renewal"});
    let fetch = |id: &str| json!({"namespaces": namespaces, "read_ceiling": ceiling, "ids": [id]});
    let timeline =
        |id: &str| json!({"namespaces": namespaces, "read_ceiling": ceiling, "anchor": id});
    let mut seen = Vec::new();
    let excerpt = |text: &str| text.chars().take(300).collect::<String>();
    for (surface, path, body) in [
        ("search", "v1/observations/search", search.clone()),
        ("pack", "v1/context/pack", pack),
        ("mcp memory_search", "mcp", tool("memory_search", search)),
    ] {
        let text = answer(path, body).await;
        if text.contains(&id) {
            seen.push(format!("{surface}: {}", excerpt(&text)));
        }
    }
    for (surface, path, hidden, absent) in [
        (
            "fetch",
            "v1/observations/fetch",
            fetch(&id),
            fetch(&missing),
        ),
        (
            "timeline",
            "v1/observations/timeline",
            timeline(&id),
            timeline(&missing),
        ),
        (
            "mcp memory_fetch",
            "mcp",
            tool("memory_fetch", fetch(&id)),
            tool("memory_fetch", fetch(&missing)),
        ),
    ] {
        let hidden = without_request_ids(&answer(path, hidden).await);
        let absent = without_request_ids(&answer(path, absent).await).replace(&missing, &id);
        if hidden != absent {
            seen.push(format!(
                "{surface}: {} vs missing {}",
                excerpt(&hidden),
                excerpt(&absent)
            ));
        }
    }
    seen
}

/// 06B.2: labels follow consumption, not the latest output, and every read surface honours them.
/// - a ceiling-6 run that consumed exactly level 4 saves at 4 into agent scope; one that consumed
///   level 6 saves at 6, whatever it recalled last; inexact accounting under ceiling 6 saves at 6
///   (the ceiling, never 0); company scope saves authored input at the ceiling (U2);
/// - recall at ceiling 6 (readers 6/8/9) sees the 4 and 6 saves, and an independent level-7
///   memory stays out;
/// - a level-3 reader finds none of the level-4 save through HTTP search, fetch, timeline,
///   context or the MCP tools, whether it reads with OpenBot's credential at ceiling 3 or holds
///   its own read_max-3 key.
#[tokio::test]
#[ignore = "needs the live memory service: scripts/memory-integration.sh"]
async fn memory_e2e_labels_follow_consumption_and_no_lower_reader_sees_them() {
    let live = live();
    let http = client(&live);
    let f = Fixture::new().await.expect("TEST_DATABASE_URL");
    provisioned(&f, &live, &http).await;
    let agent = SaveScope::Agent(f.agent);
    frozen(
        &f,
        agent,
        "ob.s.e2e-exact-4",
        run(AccessLevel::L6, AccessLevel::L4, true),
        vec![],
    )
    .await;
    frozen(
        &f,
        agent,
        "ob.s.e2e-exact-6",
        run(AccessLevel::L6, AccessLevel::L6, true),
        vec![],
    )
    .await;
    frozen(
        &f,
        agent,
        "ob.s.e2e-inexact",
        run(AccessLevel::L6, AccessLevel::L0, false),
        vec![],
    )
    .await;
    frozen(
        &f,
        SaveScope::Company,
        "ob.s.e2e-company-4",
        run(AccessLevel::L6, AccessLevel::L4, true),
        vec![],
    )
    .await;
    deliver(&f, &http).await;
    let (four, level) = saved(&f, "ob.s.e2e-exact-4").await;
    assert_eq!(level, 4, "exact consumption 4 under ceiling 6");
    let (six, level) = saved(&f, "ob.s.e2e-exact-6").await;
    assert_eq!(level, 6, "consumed 6 saves 6");
    let (fallback, level) = saved(&f, "ob.s.e2e-inexact").await;
    assert_eq!(
        level, 6,
        "missing precise consumption saves at the ceiling, never lower"
    );
    assert_eq!(
        saved(&f, "ob.s.e2e-company-4").await.1,
        6,
        "company scope: the ceiling (U2)"
    );

    // An independent client writes a level-7 memory into the same agent namespace.
    let (agent_ns, agent_credential) = ready(&f, "agent").await;
    let writer = call(
        &live,
        "admin/v1/credentials/issue",
        &live.operator,
        json!({"tenant": f.company.hyphenated().to_string(), "label": "e2e writer", "expires_at": null,
               "grants": [{"namespace": agent_ns.hyphenated().to_string(), "read_max": 9, "write_max": 9,
                           "operations": ["save", "search"]}]}),
    )
    .await["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let seven = call(
        &live,
        "v1/observations/save",
        &writer,
        json!({"idempotency_key": format!("e2e.seven.{}", f.company.simple()),
               "expires_at": (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
               "namespace": agent_ns.hyphenated().to_string(), "session_id": null,
               "context": {"producing_ceiling": 7, "consumed": {"kind": "exact", "level": 7},
                           "sources": [], "external_sources": [], "expected_access_level": null},
               "items": [{"kind": "fact", "title": "E2E level 7", "body": "E2E-seven: the renewal is due in March.",
                          "occurred_at": "2026-11-08T10:00:00Z", "expires_at": null}]}),
    )
    .await["observations"][0]["observation_id"]
        .as_str()
        .unwrap()
        .parse::<Uuid>()
        .unwrap();

    // Recall at ceiling 6 through OpenBot's own credential: 4 and 6, never 7.
    let hits = eventually("every save is searchable", async || {
        let found = http
            .search(
                &agent_credential,
                &MemorySearch {
                    namespace: NamespaceExternalId(agent_ns),
                    read_ceiling: 6,
                    query: "renewal March".into(),
                    limit: 50,
                },
            )
            .await
            .unwrap();
        let ids: Vec<Uuid> = found.items.iter().map(|hit| hit.observation_id).collect();
        [four, six, fallback]
            .iter()
            .all(|id| ids.contains(id))
            .then_some(found)
    })
    .await;
    assert!(hits.items.iter().all(|hit| hit.access_level <= 6));
    assert!(!hits.items.iter().any(|hit| hit.observation_id == seven));
    let packs = http
        .context_pack(&[ScopedPack {
            credential: &agent_credential,
            pack: MemoryPackRequest {
                namespace: NamespaceExternalId(agent_ns),
                read_ceiling: 6,
                query: Some("renewal".into()),
                max_tokens: 4096,
                max_bytes: 65_536,
            },
        }])
        .await
        .unwrap();
    assert!(
        packs[0]
            .items
            .iter()
            .all(|item| item.observation_id != seven)
    );

    // A level-3 reader: OpenBot's credential at ceiling 3, and an independent read_max-3 key.
    let below = http
        .fetch(
            &agent_credential,
            NamespaceExternalId(agent_ns),
            3,
            &[four, six, fallback],
        )
        .await
        .unwrap();
    assert!(
        below.items.is_empty(),
        "nothing above 3 is fetched at ceiling 3"
    );
    let token = agent_credential.token.expose_secret().to_owned();
    assert_eq!(
        visible_anywhere(&live, &token, agent_ns, 3, four).await,
        Vec::<String>::new(),
        "OpenBot's credential at ceiling 3"
    );
    let low = reader(&live, &f, agent_ns, 3).await;
    assert_eq!(
        visible_anywhere(&live, &low, agent_ns, 3, four).await,
        Vec::<String>::new(),
        "an independent read_max-3 key"
    );
    assert!(
        !visible_anywhere(&live, &token, agent_ns, 6, four)
            .await
            .is_empty(),
        "the probe itself finds the save at ceiling 6"
    );
}

/// 06B.2: tampered labels and missing authority fail safely against the real service. A frozen
/// intent edited after the freeze to claim consumption above its ceiling, to drop its ceiling, or
/// to cite a level-7 source, is refused by the service and saves nothing. A known level-7 source under ceiling 6 is refused
/// earlier, by OpenBot's freeze, so no intent exists.
#[tokio::test]
#[ignore = "needs the live memory service: scripts/memory-integration.sh"]
async fn memory_e2e_tampered_labels_and_missing_authority_save_nothing() {
    let live = live();
    let http = client(&live);
    let f = Fixture::new().await.expect("TEST_DATABASE_URL");
    provisioned(&f, &live, &http).await;
    let agent = SaveScope::Agent(f.agent);
    for key in [
        "ob.s.e2e-above",
        "ob.s.e2e-no-ceiling",
        "ob.s.e2e-source-7-after",
    ] {
        frozen(
            &f,
            agent,
            key,
            run(AccessLevel::L6, AccessLevel::L4, true),
            vec![],
        )
        .await;
    }
    let tamper = |key: &'static str, path: &'static [&'static str], value: Value| {
        let pool = f.pool().clone();
        async move {
            sqlx::query(
                "UPDATE memory_deliveries SET request = jsonb_set(request, $2, $3) \
                 WHERE idempotency_key = $1",
            )
            .bind(key)
            .bind(path)
            .bind(value)
            .execute(&pool)
            .await
            .unwrap();
        }
    };
    tamper(
        "ob.s.e2e-above",
        &["body", "context", "consumed"],
        json!({"kind": "exact", "level": 7}),
    )
    .await;
    // A known level-7 source written into the intent after its freeze (review B3 F3b).
    tamper(
        "ob.s.e2e-source-7-after",
        &["body", "context", "external_sources"],
        json!([{"reference": "https://example.test/level-7", "access_level": 7}]),
    )
    .await;
    tamper(
        "ob.s.e2e-no-ceiling",
        &["body", "context", "producing_ceiling"],
        Value::Null,
    )
    .await;
    // A known level-7 source under ceiling 6: OpenBot refuses it at the freeze, before any intent.
    assert!(
        freeze(
            &f,
            agent,
            "ob.s.e2e-source-7",
            run(AccessLevel::L6, AccessLevel::L4, true),
            vec![ExternalSourceRef {
                reference: "https://example.test/level-7".into(),
                access_level: 7,
            }],
        )
        .await
        .is_err()
    );
    let intents: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM memory_deliveries WHERE idempotency_key = 'ob.s.e2e-source-7'",
    )
    .fetch_one(f.pool())
    .await
    .unwrap();
    assert_eq!(intents, 0);
    deliver(&f, &http).await;
    for key in [
        "ob.s.e2e-above",
        "ob.s.e2e-no-ceiling",
        "ob.s.e2e-source-7-after",
    ] {
        let (state, result) = delivery(&f, key).await;
        assert!(state.starts_with("failed("), "{key}: {state}");
        assert!(result.is_none(), "{key}: nothing saved");
    }
    let (agent_ns, credential) = ready(&f, "agent").await;
    let everything = http
        .search(
            &credential,
            &MemorySearch {
                namespace: NamespaceExternalId(agent_ns),
                read_ceiling: 6,
                query: "renewal".into(),
                limit: 50,
            },
        )
        .await
        .unwrap();
    assert!(
        everything.items.is_empty(),
        "the service stored none of them"
    );
}

/// 06B.2 revocation, A4-9 against the real service: the operator revokes OpenBot's namespace
/// credential; the pending save's 401 queues re-verification without spending an attempt, the
/// reconciler finds the credential gone from the live listing and swaps in a new one, and the
/// save is then delivered once. (Replay of a delivered key is covered by
/// `memory_integration_live_provisioning_save_recall_capture_and_disable`.)
#[tokio::test]
#[ignore = "needs the live memory service: scripts/memory-integration.sh"]
async fn memory_e2e_a_revoked_credential_is_replaced_and_the_save_delivered_once() {
    let live = live();
    let http = client(&live);
    let f = Fixture::new().await.expect("TEST_DATABASE_URL");
    provisioned(&f, &live, &http).await;
    let (agent_ns, old) = ready(&f, "agent").await;
    call(
        &live,
        "admin/v1/credentials/revoke",
        &live.operator,
        json!({"tenant": f.company.hyphenated().to_string(), "credential_id": old.id}),
    )
    .await;
    frozen(
        &f,
        SaveScope::Agent(f.agent),
        "ob.s.e2e-revoked",
        run(AccessLevel::L6, AccessLevel::L4, true),
        vec![],
    )
    .await;
    let client: Arc<dyn MemoryClient> = Arc::new(ReverifyingClient::new(
        http.clone(),
        Arc::new(f.persistence.clone()),
    ));
    let worker = MemoryDeliveryWorker::new(Arc::new(f.persistence.clone()), client);
    assert!(worker.pass(&CancellationToken::new()).await.unwrap());
    assert_eq!(delivery(&f, "ob.s.e2e-revoked").await.0, "pending");
    reconcile_with(&f.persistence, &live, &http).await;
    let (namespace, new) = ready(&f, "agent").await;
    assert_eq!(namespace, agent_ns, "the namespace stayed the same");
    assert_ne!(new.id, old.id, "a new credential was swapped in");
    sqlx::query(
        "UPDATE memory_deliveries SET available_at = clock_timestamp() WHERE state = 'pending'",
    )
    .execute(f.pool())
    .await
    .unwrap();
    assert!(worker.pass(&CancellationToken::new()).await.unwrap());
    let (_, level) = saved(&f, "ob.s.e2e-revoked").await;
    assert_eq!(level, 4);
    let (attempts, transient): (i32, i32) = sqlx::query_as(
        "SELECT attempts, transient_retries FROM memory_deliveries \
         WHERE idempotency_key = 'ob.s.e2e-revoked'",
    )
    .fetch_one(f.pool())
    .await
    .unwrap();
    assert_eq!(
        (attempts, transient),
        (0, 1),
        "the refused attempt was transient, not counted"
    );
}
