//! `memory_search_` over PostgreSQL with the deterministic fake embedder (D7, D10): a fixed
//! corpus, both legs' predicates ahead of every limit, the iterative-scan fallback behind more
//! than 20,000 nearer ineligible parts, the partial flag, indexing state, invalid vectors and
//! model identity. Require MEMORY_TEST_DATABASE_URL.
use super::storage_tests::{
    Fixture, code, context, delete, exact, fixture, fixture_with, http, pg_router, save, tool,
};
use crate::{
    application::{
        indexer::{IndexPass, Indexer, IndexerSettings},
        test_embedder::{
            FAKE_MODEL, FakeEmbedder, FakeMode, fake_pool, fake_pool_with, fake_vector,
        },
    },
    domain::{error::ErrorCode, retrieval::normalize},
};
use axum::http::StatusCode;
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::Arc};
use uuid::Uuid;

fn item(title: &str, body: &str) -> Value {
    json!({"kind": "note", "title": title, "body": body,
           "occurred_at": "2026-11-08T10:00:00Z", "expires_at": null})
}

/// Save `items` in one request at exact `level` and return their ids in order.
async fn put(
    f: &Fixture,
    client: &crate::application::service::Principal,
    namespace: &str,
    level: i64,
    items: Vec<Value>,
) -> Vec<Uuid> {
    let mut body = save(
        &Uuid::now_v7().to_string(),
        namespace,
        context(json!(level), exact(level)),
        0,
    );
    body["items"] = Value::Array(items);
    let saved = f.run(client, "observations.save", body).await.unwrap();
    saved["observations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| serde_json::from_value(o["observation_id"].clone()).unwrap())
        .collect()
}

fn indexer(f: &Fixture) -> Indexer {
    Indexer::new(
        f.app.store().clone(),
        f.app.embeddings().clone(),
        IndexerSettings::default(),
    )
}

/// Run indexing passes until nothing is due.
async fn index_all(f: &Fixture) -> IndexPass {
    let mut total = IndexPass::default();
    loop {
        let pass = indexer(f).run_once().await.unwrap();
        if pass.claimed == 0 {
            return total;
        }
        total.claimed += pass.claimed;
        total.indexed += pass.indexed;
        total.failed += pass.failed;
        total.stale += pass.stale;
        total.abandoned += pass.abandoned;
    }
}

fn query(namespaces: &[&str], ceiling: i64, text: &str, limit: u32) -> Value {
    json!({"namespaces": namespaces, "read_ceiling": ceiling, "query": text, "limit": limit})
}

async fn search(
    f: &Fixture,
    client: &crate::application::service::Principal,
    body: Value,
) -> Value {
    f.run(client, "observations.search", body).await.unwrap()
}

fn ids(result: &Value) -> Vec<Uuid> {
    result["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| serde_json::from_value(item["observation_id"].clone()).unwrap())
        .collect()
}

async fn parts(f: &Fixture, id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM observation_parts WHERE observation_id = $1")
        .bind(id)
        .fetch_one(f.db.pool())
        .await
        .unwrap()
}

async fn make_due(f: &Fixture) {
    sqlx::query("UPDATE observations SET next_index_at = now() WHERE index_state <> 'indexed'")
        .execute(f.db.pool())
        .await
        .unwrap();
}

/// The fixed corpus: exact identifiers, paraphrases, a long note, duplicate parts, non-English
/// text and empty matches, over HTTP and MCP alike.
#[tokio::test]
async fn memory_search_fixed_corpus_recalls_ids_paraphrases_long_notes_and_languages() {
    let f = fixture().await;
    f.tenant("t", &["a", "empty"]).await;
    let (token, client) = f.client("t", &[("a", 9, 9), ("empty", 9, 9)]).await;
    let filler: Vec<String> = (0..60)
        .map(|n| {
            format!("Filler paragraph number {n} describes routine operations and nothing else.")
        })
        .collect();
    let long = format!(
        "{}\n\nThe secret ingredient is cardamom from the hills.",
        filler.join("\n\n")
    );
    let duplicated = vec!["Kubernetes pods restart after the node drains."; 120].join("\n\n");
    let corpus = put(
        &f,
        &client,
        "a",
        3,
        vec![
            item(
                "Incident INC-48213",
                "Database failover at 03:00 caused the outage.",
            ),
            item("Car maintenance", "We fixed the car brakes yesterday."),
            item("Quarterly report", &long),
            item("Cluster notes", &duplicated),
            item("Buchhaltung", "Die Rechnung wurde gestern bezahlt."),
            item("日本語のメモ", "記憶検索のテストです"),
            item("Empty body", ""),
        ],
    )
    .await;
    let [
        exact_id,
        paraphrase,
        long_id,
        duplicate,
        german,
        japanese,
        empty_body,
    ] = corpus[..]
    else {
        panic!()
    };
    // Keyword recall works before any semantic indexing.
    let pending = search(&f, &client, query(&["a"], 9, "INC-48213", 5)).await;
    assert_eq!(ids(&pending)[0], exact_id);
    assert_eq!(pending["items"][0]["index_state"], "pending");
    let pass = index_all(&f).await;
    assert_eq!((pass.indexed, pass.failed), (7, 0), "{pass:?}");
    assert!(
        parts(&f, long_id).await > 1,
        "a long note is split into parts"
    );
    assert!(parts(&f, duplicate).await > 1);
    assert_eq!(
        parts(&f, empty_body).await,
        1,
        "an empty body has its title part"
    );

    let found = search(&f, &client, query(&["a"], 9, "INC-48213", 5)).await;
    assert_eq!(ids(&found)[0], exact_id, "an exact identifier ranks first");
    assert_eq!(found["items"][0]["index_state"], "indexed");
    assert_eq!(found["exposed_level"], 3);
    assert_eq!(found["semantic"], "complete");
    // A paraphrase with no shared word is found by the semantic leg alone.
    let found = search(&f, &client, query(&["a"], 9, "automobile repair", 3)).await;
    assert_eq!(ids(&found)[0], paraphrase);
    // Text deep in a long note: its best part is the snippet, within 320 bytes.
    let found = search(&f, &client, query(&["a"], 9, "cardamom ingredient", 3)).await;
    assert_eq!(ids(&found)[0], long_id);
    let snippet = found["items"][0]["snippet"].as_str().unwrap();
    assert!(
        snippet.contains("cardamom") && snippet.len() <= 320,
        "{snippet}"
    );
    // Many matching parts of one observation: one result.
    let found = search(&f, &client, query(&["a"], 9, "Kubernetes pods restart", 7)).await;
    let listed = ids(&found);
    assert_eq!(listed[0], duplicate);
    assert_eq!(listed.iter().filter(|id| **id == duplicate).count(), 1);
    assert_eq!(listed.len(), BTreeSet::from_iter(listed.iter()).len());
    // Non-English text: German by keyword and meaning, Japanese by meaning.
    let found = search(&f, &client, query(&["a"], 9, "Rechnung bezahlt", 3)).await;
    assert_eq!(ids(&found)[0], german);
    let found = search(&f, &client, query(&["a"], 9, "記憶検索", 3)).await;
    assert_eq!(ids(&found)[0], japanese);
    // Empty matches: a blank query and a namespace with nothing eligible.
    for body in [
        query(&["a"], 9, "   ", 5),
        query(&["empty"], 9, "anything at all", 5),
    ] {
        let found = search(&f, &client, body).await;
        assert_eq!(found["items"], json!([]));
        assert_eq!(found["exposed_level"], 0);
    }
    // The bounds: limit, query length and ceiling are checked before any work.
    for bad in [
        query(&["a"], 9, "x", 0),
        query(&["a"], 9, "x", 101),
        query(&["a"], 9, &"q".repeat(1025), 5),
    ] {
        assert_eq!(
            code(f.run(&client, "observations.search", bad).await),
            ErrorCode::InvalidRequest
        );
    }
    // HTTP and MCP run the same use case and return the same body.
    let router = pg_router(&f);
    let body = query(&["a"], 9, "automobile repair", 3);
    let (status, by_http) = http(&router, "/v1/observations/search", &token, body.clone()).await;
    assert_eq!(status, StatusCode::OK);
    let by_tool = tool(&router, &token, "memory_search", body).await;
    assert_eq!(by_tool["structuredContent"], by_http);
    let (status, denied) = http(
        &router,
        "/v1/observations/search",
        &token,
        query(&["b"], 9, "x", 3),
    )
    .await;
    assert_eq!(
        (status, denied["error"]["code"].clone()),
        (StatusCode::FORBIDDEN, json!("forbidden"))
    );
    let denied = tool(&router, &token, "memory_search", query(&["b"], 9, "x", 3)).await;
    assert_eq!(denied["structuredContent"]["error"]["code"], "forbidden");
}

/// Foreign-tenant, other-namespace and above-ceiling rows identical to the query (the nearest
/// possible), deleted and expired rows: none is returned or counted, and the eligible 0/4/6
/// rows are recalled at ceiling 6, unchanged by 7/9 rows.
#[tokio::test]
async fn memory_search_filters_both_legs_before_limits_and_ceiling_six_is_unchanged_by_seven_and_nine()
 {
    let f = fixture().await;
    f.tenant("t", &["a", "b"]).await;
    f.tenant("u", &["a"]).await;
    let (_, client) = f.client("t", &[("a", 9, 9), ("b", 9, 9)]).await;
    let (_, foreigner) = f.client("u", &[("a", 9, 9)]).await;
    let text = || item("Orchard", "orchard apples harvest season");
    let mut eligible = Vec::new();
    for level in [0, 4, 6] {
        eligible.extend(put(&f, &client, "a", level, vec![text()]).await);
    }
    put(&f, &client, "b", 0, vec![text()]).await;
    put(&f, &foreigner, "a", 0, vec![text()]).await;
    let deleted = put(&f, &client, "a", 0, vec![text()]).await[0];
    f.run(
        &client,
        "observations.delete",
        delete("del", "a", deleted, 9),
    )
    .await
    .unwrap();
    let expired = put(&f, &client, "a", 0, vec![text()]).await[0];
    index_all(&f).await;
    sqlx::query("UPDATE observations SET expires_at = now() - interval '1 second' WHERE id = $1")
        .bind(expired)
        .execute(f.db.pool())
        .await
        .unwrap();
    let body = query(&["a"], 6, "orchard apples harvest", 3);
    let before = search(&f, &client, body.clone()).await;
    assert_eq!(
        BTreeSet::from_iter(ids(&before)),
        BTreeSet::from_iter(eligible.clone()),
        "{before}"
    );
    assert_eq!(before["exposed_level"], 6);
    // Rows at 7 and 9, nearest to the query in both legs, change nothing at ceiling 6.
    for level in [7, 9, 9, 7] {
        put(&f, &client, "a", level, vec![text()]).await;
    }
    index_all(&f).await;
    let after = search(&f, &client, body).await;
    assert_eq!(
        after, before,
        "levels 7-9 leave the ceiling-6 answer unchanged"
    );
    // A narrower ceiling sees fewer; the ceiling is the only widening knob.
    let four = search(&f, &client, query(&["a"], 4, "orchard apples harvest", 10)).await;
    assert_eq!(
        BTreeSet::from_iter(ids(&four)),
        BTreeSet::from_iter(eligible[..2].to_vec())
    );
    assert_eq!(four["exposed_level"], 4);
}

/// More than 20,000 ineligible parts nearer the query than a small eligible set: the iterative
/// scan stops at `max_scan_tuples`, the exact scan decides, and the eligible rows are returned.
#[tokio::test]
async fn memory_search_returns_eligible_rows_behind_twenty_thousand_nearer_ineligible_parts() {
    let f = fixture().await;
    f.tenant("t", &["a"]).await;
    f.tenant("u", &["a"]).await;
    let (_, client) = f.client("t", &[("a", 9, 9)]).await;
    let eligible = put(
        &f,
        &client,
        "a",
        2,
        vec![
            item("Savanna", "striped horses graze"),
            item("Grassland", "herds move at dawn"),
            item("Waterhole", "animals gather to drink"),
        ],
    )
    .await;
    index_all(&f).await;
    let query_vector = normalize(fake_vector("zebra quagga")).unwrap();
    // Bulk ineligible parts: 10,500 in a foreign tenant and 10,500 above the ceiling, each a
    // jittered copy of the query vector. The HNSW index is rebuilt with its migration definition.
    let pool = f.db.pool();
    sqlx::query("DROP INDEX observation_parts_embedding")
        .execute(pool)
        .await
        .unwrap();
    for (tenant, level) in [("u", 0i16), ("t", 9i16)] {
        sqlx::query(
            r#"WITH target AS (
                 SELECT n.tenant_id, n.id AS namespace_id FROM namespaces n
                   JOIN tenants t ON t.id = n.tenant_id WHERE t.external_id = $1 AND n.external_id = 'a'
               ), rows AS (
                 INSERT INTO observations (tenant_id, namespace_id, id, kind, title, body, access_level,
                        producing_ceiling, watermark_kind, watermark_level, source_max_level, origin,
                        occurred_at, index_state)
                 SELECT target.tenant_id, target.namespace_id, gen_random_uuid(), 'note', 'bulk', 'bulk',
                        $2, $2, 'exact', $2, 0, 'explicit', now(), 'indexed'
                   FROM target, generate_series(1, 10500)
                 RETURNING tenant_id, namespace_id, id, access_level, revision)
               INSERT INTO observation_parts (tenant_id, observation_id, namespace_id, access_level,
                      revision, ordinal, byte_start, byte_end, model_id, embedding)
               SELECT r.tenant_id, r.id, r.namespace_id, r.access_level, r.revision, 0, 0, 4, $4,
                      (SELECT array_agg(q.v + (random()::real - 0.5) * 0.002 ORDER BY q.i)
                         FROM unnest($3::real[]) WITH ORDINALITY AS q(v, i)
                        WHERE r.id IS NOT NULL)::vector
                 FROM rows r"#,
        )
        .bind(tenant)
        .bind(level)
        .bind(&query_vector)
        .bind(FAKE_MODEL)
        .execute(pool)
        .await
        .unwrap();
    }
    sqlx::query(
        "CREATE INDEX observation_parts_embedding ON observation_parts \
         USING hnsw (embedding vector_cosine_ops) WITH (m = 16, ef_construction = 64)",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("ANALYZE").execute(pool).await.unwrap();
    let nearer: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM observation_parts WHERE (embedding <=> $1::real[]::vector) < \
         (SELECT min(embedding <=> $1::real[]::vector) FROM observation_parts WHERE observation_id = ANY($2))",
    )
    .bind(&query_vector)
    .bind(&eligible)
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(
        nearer > 20_000,
        "{nearer} ineligible parts are nearer than any eligible one"
    );
    let found = search(&f, &client, query(&["a"], 6, "zebra quagga", 3)).await;
    assert_eq!(
        BTreeSet::from_iter(ids(&found)),
        BTreeSet::from_iter(eligible),
        "{found}"
    );
    assert_eq!(found["semantic"], "complete");
    assert_eq!(found["exposed_level"], 2);
}

/// Above the exact-scan cap the semantic leg says `partial` instead of being silently short.
#[tokio::test]
async fn memory_search_reports_partial_semantic_results_above_the_exact_cap() {
    let f = fixture_with(fake_pool(), Some(2)).await;
    f.tenant("t", &["a"]).await;
    let (_, client) = f.client("t", &[("a", 9, 9)]).await;
    put(
        &f,
        &client,
        "a",
        1,
        vec![
            item("One", "alpha"),
            item("Two", "beta"),
            item("Three", "gamma"),
        ],
    )
    .await;
    index_all(&f).await;
    let before = super::semantic_partial_total();
    let found = search(&f, &client, query(&["a"], 9, "alpha", 1)).await;
    assert_eq!(found["semantic"], "partial");
    assert_eq!(found["items"].as_array().unwrap().len(), 1);
    assert!(super::semantic_partial_total() > before);
    // Under the cap the same search is complete.
    let f = fixture_with(fake_pool(), Some(3)).await;
    f.tenant("t", &["a"]).await;
    let (_, client) = f.client("t", &[("a", 9, 9)]).await;
    put(
        &f,
        &client,
        "a",
        1,
        vec![
            item("One", "alpha"),
            item("Two", "beta"),
            item("Three", "gamma"),
        ],
    )
    .await;
    index_all(&f).await;
    assert_eq!(
        search(&f, &client, query(&["a"], 9, "alpha", 1)).await["semantic"],
        "complete"
    );
}

/// Indexing failures stay visible and never block keyword recall; invalid vectors never reach
/// the database; a query embedding failure is 503; terminal failures need an operator requeue.
#[tokio::test]
async fn memory_search_failed_indexing_and_invalid_vectors_keep_keyword_recall() {
    let fake = Arc::new(FakeEmbedder::new(FAKE_MODEL));
    let f = fixture_with(fake_pool_with(fake.clone()), None).await;
    f.tenant("t", &["a"]).await;
    let (_, client) = f.client("t", &[("a", 9, 9)]).await;
    let id = put(
        &f,
        &client,
        "a",
        1,
        vec![item("Gearbox telemetry", "torque sensor drift")],
    )
    .await[0];
    let state = |f: &Fixture| {
        let pool = f.db.pool().clone();
        async move {
            sqlx::query_as::<_, (String, Option<String>, i32)>(
                "SELECT index_state, index_error, index_attempts FROM observations WHERE id = $1",
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap()
        }
    };
    for (mode, error) in [
        (FakeMode::Fail, "embedding_unavailable"),
        (FakeMode::Nan, "invalid_vector"),
        (FakeMode::Zero, "invalid_vector"),
    ] {
        fake.set(mode);
        make_due(&f).await;
        let pass = indexer(&f).run_once().await.unwrap();
        assert_eq!(pass.failed, 1, "{mode:?}");
        let (index_state, index_error, _) = state(&f).await;
        assert_eq!(
            (index_state.as_str(), index_error.as_deref()),
            ("failed", Some(error))
        );
        assert_eq!(parts(&f, id).await, 0, "no invalid vector is stored");
        // Not due again until its backoff passes.
        assert_eq!(indexer(&f).run_once().await.unwrap().claimed, 0);
        if mode == FakeMode::Fail {
            // A query embedding failure is 503, never a keyword-only answer.
            assert_eq!(
                code(
                    f.run(
                        &client,
                        "observations.search",
                        query(&["a"], 9, "torque", 5)
                    )
                    .await
                ),
                ErrorCode::Unavailable
            );
        }
    }
    fake.set(FakeMode::Ok);
    let found = search(&f, &client, query(&["a"], 9, "torque", 5)).await;
    assert_eq!(ids(&found), vec![id]);
    assert_eq!(found["items"][0]["index_state"], "failed");
    // The eighth failed attempt is terminal; an operator requeue indexes the saved text.
    sqlx::query("UPDATE observations SET index_attempts = 7, next_index_at = now() WHERE id = $1")
        .bind(id)
        .execute(f.db.pool())
        .await
        .unwrap();
    fake.set(FakeMode::Fail);
    assert_eq!(indexer(&f).run_once().await.unwrap().failed, 1);
    assert_eq!(state(&f).await.2, 8);
    fake.set(FakeMode::Ok);
    make_due(&f).await;
    assert_eq!(indexer(&f).run_once().await.unwrap().claimed, 0, "terminal");
    assert_eq!(f.app.store().reindex_failed().await.unwrap(), 1);
    assert_eq!(index_all(&f).await.indexed, 1);
    assert_eq!(state(&f).await, ("indexed".into(), None, 0));
    assert_eq!(
        f.scalar("SELECT count(*) FROM audit_events WHERE action = 'index.reindex_failed'")
            .await,
        1
    );
    // The database itself refuses wrong widths and non-finite values.
    let pool = f.db.pool();
    for bad in [
        format!("[{}]", vec!["0.1"; 383].join(",")),
        format!("[NaN{}]", ",0.1".repeat(383)),
    ] {
        let result = sqlx::query(
            "INSERT INTO observation_parts (tenant_id, observation_id, namespace_id, access_level, \
             revision, ordinal, byte_start, byte_end, model_id, embedding) \
             SELECT tenant_id, id, namespace_id, access_level, revision, 5, 0, 1, 'x', $2::vector \
               FROM observations WHERE id = $1",
        )
        .bind(id)
        .bind(&bad)
        .execute(pool)
        .await;
        assert!(result.is_err());
    }
}

/// Vectors of another model are never compared, and a database indexed with another model
/// refuses the configured one; a missing vector capability is refused explicitly.
#[tokio::test]
async fn memory_search_never_compares_vectors_of_another_model() {
    let f = fixture().await;
    f.tenant("t", &["a"]).await;
    let (_, client) = f.client("t", &[("a", 9, 9)]).await;
    let ours = put(&f, &client, "a", 1, vec![item("Ours", "lantern festival")]).await[0];
    index_all(&f).await;
    let theirs = put(&f, &client, "a", 1, vec![item("Theirs", "unrelated words")]).await[0];
    // A part of another model, identical to the query vector: never compared.
    sqlx::query(
        "INSERT INTO observation_parts (tenant_id, observation_id, namespace_id, access_level, \
         revision, ordinal, byte_start, byte_end, model_id, embedding) \
         SELECT tenant_id, id, namespace_id, access_level, revision, 0, 0, 5, 'other/model@1', \
                $2::real[]::vector FROM observations WHERE id = $1",
    )
    .bind(theirs)
    .bind(normalize(fake_vector("paper lantern")).unwrap())
    .execute(f.db.pool())
    .await
    .unwrap();
    let found = search(&f, &client, query(&["a"], 9, "paper lantern", 5)).await;
    assert_eq!(ids(&found), vec![ours], "{found}");
    let store = f.app.store();
    store.adopt_embedding_model(FAKE_MODEL).await.unwrap();
    let refused = store
        .adopt_embedding_model("other/model@1")
        .await
        .unwrap_err();
    assert!(
        refused.message.contains("reset the service database"),
        "{}",
        refused.message
    );
    store.vector_capability().await.unwrap();
    sqlx::query("ALTER TABLE observation_parts DROP COLUMN embedding")
        .execute(f.db.pool())
        .await
        .unwrap();
    let missing = store.vector_capability().await.unwrap_err();
    assert!(
        missing.message.contains("vector capability is missing"),
        "{}",
        missing.message
    );
}

/// The pinned real model end to end: save, index, and retrieve a small multilingual corpus by
/// cross-language paraphrase, exact identifier and a fact deep in a long note. It needs the
/// prepared model (scripts/prepare-model.py --download) and fails, never skips, without it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn memory_search_real_model_retrieval_acceptance() {
    let directory = std::env::var_os("MEMORY_MODEL_DIR").map_or_else(
        || std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/model"),
        std::path::PathBuf::from,
    );
    let loaded = tokio::task::spawn_blocking(move || crate::adapters::embedding::load(&directory))
        .await
        .unwrap()
        .unwrap_or_else(|error| {
            panic!(
                "the real-model acceptance check needs the prepared model; run \
                 `python3 -I scripts/prepare-model.py --download` (build network only): {error:#}"
            )
        });
    let pool = Arc::new(loaded.pool().unwrap());
    let f = fixture_with(pool, None).await;
    f.tenant("t", &["a"]).await;
    let (_, client) = f.client("t", &[("a", 9, 9)]).await;
    let filler: Vec<String> = (0..30)
        .map(|n| {
            format!("Observation log {n}: the telescope tracked the orbit of a distant comet.")
        })
        .collect();
    let long = format!(
        "{}\n\nOur vendor for coffee beans is a small roastery located in Ljubljana.",
        filler.join("\n\n")
    );
    let corpus = put(
        &f,
        &client,
        "a",
        2,
        vec![
            item("Rückgaberecht", "Kunden können Produkte innerhalb von dreißig Tagen zurückgeben und erhalten eine vollständige Rückerstattung."),
            item("Garanzia", "Tutti i prodotti hanno una garanzia di ventiquattro mesi dalla consegna."),
            item("Envíos nacionales", "Los pedidos nacionales llegan en tres a cinco días laborables."),
            item("Deployment incident AX-4401", "The AX-4401 rollout failed because the database migration timed out."),
            item("Office plants", "The ficus in the lobby needs watering twice a week."),
            item("Team minutes", &long),
        ],
    )
    .await;
    let pass = index_all(&f).await;
    assert_eq!((pass.indexed, pass.failed), (6, 0), "{pass:?}");
    assert!(
        parts(&f, corpus[5]).await > 1,
        "the long note has several parts"
    );
    let model: String = sqlx::query_scalar("SELECT DISTINCT model_id FROM observation_parts")
        .fetch_one(f.db.pool())
        .await
        .unwrap();
    assert!(
        model.starts_with("intfloat/multilingual-e5-small@614241f6"),
        "{model}"
    );
    for (text, expected) in [
        (
            "How many days do customers have to return products for a refund?",
            0,
        ),
        ("How long is the warranty on products?", 1),
        ("How quickly does domestic shipping arrive?", 2),
        ("AX-4401", 3),
        ("Which city is our coffee supplier in?", 5),
    ] {
        let found = search(&f, &client, query(&["a"], 9, text, 3)).await;
        assert_eq!(
            ids(&found).first(),
            Some(&corpus[expected]),
            "{text}: {found}"
        );
    }
    let found = search(
        &f,
        &client,
        query(&["a"], 9, "Which city is our coffee supplier in?", 1),
    )
    .await;
    let snippet = found["items"][0]["snippet"].as_str().unwrap();
    assert!(snippet.contains("Ljubljana"), "{snippet}");
}

/// F1 (review p4-code-r1): exact identifiers stay useful inside ordinary sentences, including
/// digit-free, snake_case, dotted, e-mail and capitalised ones, while a query of function words
/// does not match everything. Probed on the keyword leg itself, in one snapshot.
#[tokio::test]
async fn memory_search_keyword_leg_keeps_identifiers_inside_sentences() {
    let f = fixture().await;
    f.tenant("t", &["a"]).await;
    let (_, client) = f.client("t", &[("a", 9, 9)]).await;
    let mut notes = vec![
        item("Storage", "The PgStore adapter owns every statement."),
        item("Retries", "Send the same idempotency_key with every retry."),
        item("Config", "The value lives in foo.bar.baz for now."),
        item("Mail", "Alerts go to ops@example.com overnight."),
        item(
            "Network",
            "Clients see ERR_CONN_RESET when the proxy restarts.",
        ),
        item("Release", "A regression shipped in v1.2.3 last week."),
    ];
    for n in 0..20 {
        notes.push(item(
            "Filler",
            &format!("Where is the meeting and what do I do about it, my item {n}?"),
        ));
    }
    let ids = put(&f, &client, "a", 1, notes).await;
    let (tenant, namespace): (Uuid, Uuid) = sqlx::query_as(
        "SELECT n.tenant_id, n.id FROM namespaces n JOIN tenants t ON t.id = n.tenant_id \
         WHERE t.external_id = 't' AND n.external_id = 'a'",
    )
    .fetch_one(f.db.pool())
    .await
    .unwrap();
    let read = crate::application::ports::ClientRead {
        tenant_id: tenant,
        namespace_ids: vec![namespace],
        read_ceiling: crate::domain::access_level::AccessLevel::new(9).unwrap(),
    };
    let mut db = f.db.pool().acquire().await.unwrap();
    let mut keyword = async |text: &str| -> Vec<Uuid> {
        let query = crate::application::ports::SearchQuery {
            text: text.to_owned(),
            identifiers: crate::domain::retrieval::identifier_tokens(text),
            vector: Vec::new(),
            model_id: FAKE_MODEL.into(),
            limit: 50,
        };
        let Ok(found) = super::search::keyword_leg(&mut db, &read, &query, 200).await else {
            panic!("keyword leg failed for {text}");
        };
        found.into_iter().map(|ranked| ranked.id).collect()
    };
    for (text, expected) in [
        ("where is PgStore used", 0),
        ("which idempotency_key do I send", 1),
        ("what is in foo.bar.baz", 2),
        ("who gets mail at ops@example.com", 3),
        ("why ERR_CONN_RESET happens", 4),
        ("error on v1.2.3", 5),
        ("PgStore", 0),
    ] {
        assert_eq!(keyword(text).await, vec![ids[expected]], "{text}");
    }
    // Function words alone: only rows holding all of them, never the whole namespace.
    let common = keyword("where is the and what do I").await;
    assert_eq!(
        common.len(),
        20,
        "only the filler holds every one of those words"
    );
    assert!(keyword("is a of the to").await.is_empty());
    // Selectivity guards (review p4-code-r2 F1a): capitalised common words are not identifier
    // phrases, so these never match the filler wholesale.
    for text in ["how do I reset IT passwords", "HOW DO I RESET MY PASSWORD"] {
        assert!(keyword(text).await.is_empty(), "{text}");
    }
    // A blank query and an identifier-free query that no row holds entirely match nothing.
    assert!(keyword("   ").await.is_empty());
    assert!(keyword("where is the cafeteria").await.is_empty());
}
