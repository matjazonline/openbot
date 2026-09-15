use super::super::views::THREAD_TASK_LOOKUP_SQL;
use super::*;
use std::collections::HashMap;

struct Turn {
    id: CanonicalMessageId,
    correlation: Uuid,
}

async fn turn(fixture: &Fixture, body: &str, agent_id: Uuid) -> Turn {
    let author = MessageAuthorWrite::Agent(AgentAuthor {
        agent_id,
        display_label: "Triage Agent".into(),
    });
    let message = fixture
        .persistence
        .create_message(&internal_message(fixture.thread.id, body, author))
        .await
        .unwrap();
    Turn {
        id: message.canonical_id,
        correlation: message.correlation_id.as_uuid(),
    }
}

struct ExpectedLink {
    message: CanonicalMessageId,
    task: Option<Uuid>,
}

fn assert_links(views: &[ThreadMessageView], expected: &[ExpectedLink]) {
    for link in expected {
        let view = views
            .iter()
            .find(|view| view.canonical_id == link.message)
            .unwrap();
        assert_eq!(view.task_id, link.task, "task for {}", link.message);
    }
}

async fn assert_every_reader(fixture: &Fixture, expected: &[ExpectedLink]) {
    let views = fixture
        .persistence
        .list_thread_message_views(fixture.thread.id)
        .await
        .unwrap();
    assert_links(&views, expected);
    let streamed = fixture
        .persistence
        .list_thread_message_views_after(fixture.thread.id, None, THREAD_HISTORY_LIMIT)
        .await
        .unwrap();
    assert_links(&streamed, expected);
    for link in expected {
        let view = fixture
            .persistence
            .get_thread_message_view(fixture.thread.id, link.message)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(view.task_id, link.task);
    }
}

async fn seed_follow_ups(fixture: &Fixture, correlations: &[Uuid]) {
    sqlx::query(
        r#"INSERT INTO background_tasks
               (id, company_id, channel_id, thread_id, correlation_id, task_type, status, created_at)
           SELECT gen_random_uuid(), $1, $2, $3, correlation.id, 'outreach_follow_up', 'completed',
                  CURRENT_TIMESTAMP + follow_up.ordinal * INTERVAL '1 second'
           FROM unnest($4::uuid[]) AS correlation (id)
           CROSS JOIN generate_series(1, 1200) AS follow_up (ordinal)"#,
    )
    .bind(fixture.company_id)
    .bind(fixture.channel_id)
    .bind(fixture.thread.id)
    .bind(correlations)
    .execute(&fixture.pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn large_correlations_cannot_hide_exact_matches_or_main_runs() {
    let Some(fixture) = Fixture::isolated("task_lookup_overflow").await else {
        return;
    };
    let agent = fixture.agent().await;
    let exact = turn(&fixture, "Exact match", agent).await;
    let correlated = turn(&fixture, "Correlation match", agent).await;
    let unmatched = turn(&fixture, "No task", agent).await;
    let old = Utc::now() - chrono::Duration::days(1);
    // An exact match wins even if it is not a main run and its correlation differs.
    let direct = seed_thread_task(
        &fixture,
        "outreach_follow_up",
        Uuid::new_v4(),
        Some(exact.id.as_uuid()),
        old,
    )
    .await;
    seed_thread_task(
        &fixture,
        "email_agent_dispatch",
        exact.correlation,
        None,
        old,
    )
    .await;
    let main = seed_thread_task(
        &fixture,
        "scheduled_agent_run",
        correlated.correlation,
        None,
        old,
    )
    .await;
    // The old global LIMIT 800 discards both correct answers before matching starts.
    seed_follow_ups(&fixture, &[exact.correlation, correlated.correlation]).await;
    assert_every_reader(
        &fixture,
        &[
            ExpectedLink {
                message: exact.id,
                task: Some(direct),
            },
            ExpectedLink {
                message: correlated.id,
                task: Some(main),
            },
            ExpectedLink {
                message: unmatched.id,
                task: None,
            },
        ],
    )
    .await;
    fixture.cleanup().await;
}

#[tokio::test]
async fn fallback_preserves_main_run_recency_and_uuid_ties() {
    let Some(fixture) = Fixture::isolated("task_lookup_order").await else {
        return;
    };
    let agent = fixture.agent().await;
    let main_turn = turn(&fixture, "Main run tie", agent).await;
    let plain_turn = turn(&fixture, "Follow-up tie", agent).await;
    let now = Utc::now();
    let mut expected = Vec::new();
    for (turn, kinds) in [
        (&main_turn, ["email_agent_dispatch", "scheduled_agent_run"]),
        (&plain_turn, ["outreach_follow_up", "outreach_follow_up"]),
    ] {
        seed_thread_task(
            &fixture,
            kinds[0],
            turn.correlation,
            None,
            now - chrono::Duration::days(1),
        )
        .await;
        let first = seed_thread_task(&fixture, kinds[0], turn.correlation, None, now).await;
        let second = seed_thread_task(&fixture, kinds[1], turn.correlation, None, now).await;
        expected.push(ExpectedLink {
            message: turn.id,
            task: Some(first.min(second)),
        });
    }
    // A newer follow-up still loses to either main-run type.
    seed_thread_task(
        &fixture,
        "outreach_follow_up",
        main_turn.correlation,
        None,
        now + chrono::Duration::days(1),
    )
    .await;
    assert_every_reader(&fixture, &expected).await;
    fixture.cleanup().await;
}

async fn lookup(
    fixture: &Fixture,
    company_id: Uuid,
    messages: &[Uuid],
    correlations: &[Uuid],
) -> HashMap<Uuid, Option<Uuid>> {
    sqlx::query_as::<_, (Uuid, Option<Uuid>)>(THREAD_TASK_LOOKUP_SQL)
        .bind(company_id)
        .bind(fixture.thread.id)
        .bind(messages)
        .bind(correlations)
        .fetch_all(&fixture.pool)
        .await
        .unwrap()
        .into_iter()
        .collect()
}

#[tokio::test]
async fn neither_lookup_path_crosses_the_company_or_thread_scope() {
    let Some(fixture) = Fixture::isolated("task_lookup_scope").await else {
        return;
    };
    let message = turn(&fixture, "Scoped task", fixture.agent().await).await;
    let now = Utc::now();
    let direct = seed_thread_task(
        &fixture,
        "email_agent_dispatch",
        message.correlation,
        Some(message.id.as_uuid()),
        now,
    )
    .await;
    let fallback = seed_thread_task(
        &fixture,
        "scheduled_agent_run",
        message.correlation,
        None,
        now,
    )
    .await;
    let foreign = fixture.foreign_company().await;
    let messages = [message.id.as_uuid(), Uuid::new_v4()];
    let correlations = [message.correlation; 2];
    assert!(
        lookup(&fixture, foreign, &messages, &correlations)
            .await
            .values()
            .all(Option::is_none)
    );
    let matched = lookup(&fixture, fixture.company_id, &messages, &correlations).await;
    assert_eq!(matched[&messages[0]], Some(direct));
    let other_thread = fixture
        .extra_thread(fixture.channel_id, "Other thread")
        .await;
    sqlx::query("UPDATE background_tasks SET thread_id = $2 WHERE id = $1")
        .bind(direct)
        .bind(other_thread.id)
        .execute(&fixture.pool)
        .await
        .unwrap();
    let matched = lookup(&fixture, fixture.company_id, &messages, &correlations).await;
    assert_eq!(
        matched[&messages[0]],
        Some(fallback),
        "a direct match in another thread is ignored"
    );
    sqlx::query("UPDATE background_tasks SET thread_id = $2 WHERE id = $1")
        .bind(fallback)
        .bind(other_thread.id)
        .execute(&fixture.pool)
        .await
        .unwrap();
    assert!(
        lookup(&fixture, fixture.company_id, &messages, &correlations)
            .await
            .values()
            .all(Option::is_none)
    );
    assert!(
        lookup(&fixture, fixture.company_id, &[], &[])
            .await
            .is_empty()
    );
    fixture.cleanup().await;
}

fn plan_nodes<'a>(plan: &'a serde_json::Value, nodes: &mut Vec<&'a serde_json::Value>) {
    nodes.push(plan);
    if let Some(children) = plan["Plans"].as_array() {
        for child in children {
            plan_nodes(child, nodes);
        }
    }
}

async fn seed_skewed_history(fixture: &Fixture) {
    // Many one-task correlations make the generic average tiny despite the hot correlation.
    // Without the covering index, that estimate favors the old index followed by a large sort.
    sqlx::query(
        r#"INSERT INTO background_tasks
               (id, company_id, channel_id, thread_id, correlation_id, task_type, status)
           SELECT gen_random_uuid(), $1, $2, $3, gen_random_uuid(), 'history', 'completed'
           FROM generate_series(1, 10000)"#,
    )
    .bind(fixture.company_id)
    .bind(fixture.channel_id)
    .bind(fixture.thread.id)
    .execute(&fixture.pool)
    .await
    .unwrap();
    // Model retained history after vacuum has established visibility-map coverage. Freshly
    // inserted, unvacuumed history can select a different plan; see the query-evidence document.
    sqlx::query("VACUUM (ANALYZE) background_tasks")
        .execute(&fixture.pool)
        .await
        .unwrap();
}

async fn explain_lookup(
    fixture: &Fixture,
    messages: &[Uuid],
    correlations: &[Uuid],
) -> serde_json::Value {
    sqlx::query_scalar(&format!(
        "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) {THREAD_TASK_LOOKUP_SQL}"
    ))
    .bind(fixture.company_id)
    .bind(fixture.thread.id)
    .bind(messages)
    .bind(correlations)
    .fetch_one(&fixture.pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn a_full_message_page_uses_bounded_index_probes_on_skewed_history() {
    let Some(fixture) = Fixture::isolated("task_lookup_plan").await else {
        return;
    };
    let exact = turn(
        &fixture,
        "Exact match skips fallback",
        fixture.agent().await,
    )
    .await;
    let direct = seed_thread_task(
        &fixture,
        "outreach_follow_up",
        Uuid::new_v4(),
        Some(exact.id.as_uuid()),
        Utc::now(),
    )
    .await;
    let hot = Uuid::new_v4();
    let main = seed_thread_task(&fixture, "scheduled_agent_run", hot, None, Utc::now()).await;
    seed_follow_ups(&fixture, &[hot]).await;
    seed_skewed_history(&fixture).await;
    let mut messages: Vec<Uuid> = (0..THREAD_HISTORY_LIMIT).map(|_| Uuid::new_v4()).collect();
    messages[0] = exact.id.as_uuid();
    let mut correlations = vec![hot; THREAD_HISTORY_LIMIT];
    correlations[THREAD_HISTORY_LIMIT - 1] = Uuid::new_v4();
    let matched = lookup(&fixture, fixture.company_id, &messages, &correlations).await;
    assert_eq!(matched.len(), THREAD_HISTORY_LIMIT);
    assert_eq!(matched[&messages[0]], Some(direct));
    for message in &messages[1..THREAD_HISTORY_LIMIT - 1] {
        assert_eq!(matched[message], Some(main));
    }
    assert_eq!(matched[&messages[THREAD_HISTORY_LIMIT - 1]], None);
    let explained = explain_lookup(&fixture, &messages, &correlations).await;
    // PostgreSQL 18 reports row counts as fractions (`200.00`), earlier releases as integers.
    assert_eq!(
        explained[0]["Plan"]["Actual Rows"].as_f64(),
        Some(THREAD_HISTORY_LIMIT as f64),
        "{explained}"
    );
    let mut nodes = Vec::new();
    plan_nodes(&explained[0]["Plan"], &mut nodes);
    assert!(
        !nodes
            .iter()
            .any(|node| node["Node Type"] == "Sort" || node["Node Type"] == "Incremental Sort"),
        "matching must not sort retained history: {explained}"
    );
    let fallback = nodes
        .iter()
        .find(|node| node["Index Name"] == "background_tasks_thread_correlation_match_idx")
        .expect("the fallback uses the index matching its scope and preference order");
    assert!(
        fallback["Actual Rows"].as_f64().unwrap() <= 1.0,
        "{explained}"
    );
    assert!(
        fallback["Actual Loops"].as_u64().unwrap() <= (THREAD_HISTORY_LIMIT - 1) as u64,
        "an exact match does not probe the fallback: {explained}"
    );
    fixture.cleanup().await;
}
