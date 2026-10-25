//! Tenancy: every company route refuses a stranger and a plain member, an owner naming another
//! tenant's records finds nothing, and an admin sees only the channels it may. The decision
//! routes have their own tenancy checks in `decision_tests`.
use super::*;

/// Every company route's answer to `user` acting in `company` on `ids`, by route name: the 22
/// definition, template, binding, run, tool and grant routes.
async fn probe(
    f: &Fixture,
    user: Uuid,
    company: Uuid,
    ids: Ids,
) -> Vec<(&'static str, StatusCode)> {
    let s = || f.state();
    let control = || body(json!({"expected_revision": 1, "command_key": "probe"}));
    vec![
        (
            "definitions.list",
            answer(
                definitions::list(s(), by(user), Path(company), Query(ListQuery::default())).await,
            )
            .await
            .0,
        ),
        (
            "definitions.read",
            answer(definitions::read(s(), by(user), Path((company, ids.workflow))).await)
                .await
                .0,
        ),
        (
            "definitions.save",
            f.save(
                user,
                company,
                ids.workflow,
                Some(1),
                &example("data.map", ids.workflow),
            )
            .await
            .0,
        ),
        (
            "definitions.validate",
            answer(
                definitions::validate(
                    s(),
                    by(user),
                    Path((company, ids.workflow)),
                    body(json!({"expected_revision": 1})),
                )
                .await,
            )
            .await
            .0,
        ),
        (
            "definitions.publish",
            f.publish(user, company, ids.workflow, 1, "probe").await.0,
        ),
        (
            "definitions.archive",
            answer(
                definitions::archive(
                    s(),
                    by(user),
                    Path((company, ids.workflow)),
                    body(json!({"expected_revision": 1})),
                )
                .await,
            )
            .await
            .0,
        ),
        (
            "templates.list",
            answer(definitions::templates(s(), by(user), Path(company)).await)
                .await
                .0,
        ),
        (
            "templates.copy",
            answer(
                definitions::copy_template(
                    s(),
                    by(user),
                    Path((
                        company,
                        crate::application::workflow::templates::AUTONOMOUS_RESPONSE.to_owned(),
                    )),
                    body(json!({"revision": 1, "command_key": "probe"})),
                )
                .await,
            )
            .await
            .0,
        ),
        (
            "bindings.list",
            answer(bindings::list(s(), by(user), Path(company), Query(ListQuery::default())).await)
                .await
                .0,
        ),
        (
            "bindings.read",
            answer(bindings::read(s(), by(user), Path((company, ids.binding))).await)
                .await
                .0,
        ),
        (
            "bindings.configure",
            f.configure(user, company, ids.binding, Some(1), ids.version)
                .await
                .0,
        ),
        (
            "bindings.activate",
            f.activate(user, company, ids.binding, 1).await.0,
        ),
        (
            "bindings.deactivate",
            answer(
                bindings::deactivate(
                    s(),
                    by(user),
                    Path((company, ids.binding)),
                    body(json!({"expected_state_revision": 1})),
                )
                .await,
            )
            .await
            .0,
        ),
        (
            "runs.start",
            f.start(user, company, ids.binding, "probe", json!({}))
                .await
                .0,
        ),
        (
            "runs.list",
            answer(
                bindings::runs(
                    s(),
                    by(user),
                    Path(company),
                    Query(bindings::RunsQuery::default()),
                )
                .await,
            )
            .await
            .0,
        ),
        ("runs.read", f.run(user, company, ids.run).await.0),
        (
            "runs.cancel",
            answer(bindings::cancel(s(), by(user), Path((company, ids.run)), control()).await)
                .await
                .0,
        ),
        (
            "runs.retry",
            answer(bindings::retry(s(), by(user), Path((company, ids.run)), control()).await)
                .await
                .0,
        ),
        (
            "tools.list",
            answer(tools::list(s(), by(user), Path(company)).await)
                .await
                .0,
        ),
        (
            "tools.register",
            answer(
                tools::register(
                    s(),
                    by(user),
                    Path((company, "probe.tool".to_owned())),
                    body(json!({"workflow_id": ids.workflow, "version_id": ids.version})),
                )
                .await,
            )
            .await
            .0,
        ),
        (
            "agents.grant",
            answer(
                tools::grant(
                    s(),
                    by(user),
                    Path((company, ids.agent)),
                    body(json!({"expected": [], "tools": ["probe.tool"]})),
                )
                .await,
            )
            .await
            .0,
        ),
        (
            "tools.revoke",
            answer(
                tools::revoke(
                    s(),
                    by(user),
                    Path((company, "probe.tool".to_owned())),
                    body(json!({"expected_revision": 1})),
                )
                .await,
            )
            .await
            .0,
        ),
    ]
}

fn assert_hidden(answers: Vec<(&'static str, StatusCode)>, who: &str) {
    for (route, status) in answers {
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "{route} answered {who} with {status}"
        );
    }
}

/// HTTP AGENTS.md: every route refuses a stranger and a plain member with 404, and an owner
/// naming another tenant's records under their own company finds nothing and changes nothing.
#[tokio::test]
async fn workflow_routes_refuse_other_tenants_and_members() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let ours = f.resources(f.owner, f.company).await;
    let theirs = f.resources(f.stranger, f.other).await;
    assert_hidden(probe(&f, f.stranger, f.company, ours).await, "a stranger");
    assert_hidden(probe(&f, f.member, f.company, ours).await, "a member");
    for (route, status) in probe(&f, f.owner, f.company, theirs).await {
        let scoped = match route {
            // Company-wide routes answer the owner's own company.
            "definitions.list" | "templates.list" | "bindings.list" | "runs.list"
            | "tools.list" => status == StatusCode::OK,
            // The shipped catalogue is every company's; a copy names no foreign record.
            "templates.copy" => matches!(
                status,
                StatusCode::CREATED | StatusCode::CONFLICT | StatusCode::NOT_FOUND
            ),
            // A foreign version cannot be bound or registered under this company.
            _ => matches!(status, StatusCode::NOT_FOUND | StatusCode::CONFLICT),
        };
        assert!(
            scoped,
            "{route} answered the owner with {status} for foreign ids"
        );
    }
    let (status, foreign) = f.run(f.stranger, f.other, theirs.run).await;
    assert_eq!(status, StatusCode::OK, "{foreign}");
    assert_eq!(foreign["state"], "queued", "{foreign}");
    let (_, read) = answer(
        definitions::read(f.state(), by(f.stranger), Path((f.other, theirs.workflow))).await,
    )
    .await;
    assert_eq!(read["revision"], 1, "{read}");
    assert_eq!(read["archived"], false, "{read}");
    let (_, binding) =
        answer(bindings::read(f.state(), by(f.stranger), Path((f.other, theirs.binding))).await)
            .await;
    assert_eq!(binding["active"], true, "{binding}");
    assert_eq!(binding["state_revision"], 2, "{binding}");
    let (_, listed) = answer(
        bindings::list(
            f.state(),
            by(f.owner),
            Path(f.company),
            Query(ListQuery::default()),
        )
        .await,
    )
    .await;
    assert_eq!(Fixture::items(&listed).0.len(), 1, "{listed}");
    let stored: Vec<String> = sqlx::query_scalar("SELECT workflow_tools FROM agents WHERE id = $1")
        .bind(theirs.agent)
        .fetch_one(f.persistence.pool())
        .await
        .unwrap();
    assert!(stored.is_empty(), "{stored:?}");
}

/// An admin manages the company's workflows but not a channel it is not allowed on: listings
/// leave that channel's binding and runs out, and reading or starting them is not found.
#[tokio::test]
async fn workflow_listings_hide_channels_an_admin_cannot_see() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let (owner, company) = (f.owner, f.company);
    let (_, version) = f
        .published(owner, company, example("data.map", Uuid::new_v4()))
        .await;
    let channel = ChannelPersistence::create(
        &f.persistence,
        company,
        ChannelWrite {
            name: "Board".into(),
            slug: "board".into(),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap()
    .id;
    sqlx::query("UPDATE channels SET access_mode = 'allowlist' WHERE id = $1")
        .bind(channel)
        .execute(f.persistence.pool())
        .await
        .unwrap();
    let hidden = Uuid::new_v4();
    let (status, configured) = f
        .configure_on(
            owner,
            company,
            hidden,
            None,
            version,
            json!({"kind": "channel", "channel_id": channel}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{configured}");
    let revision = configured["state_revision"].as_u64().unwrap();
    assert_eq!(
        f.activate(owner, company, hidden, revision).await.0,
        StatusCode::OK
    );
    let shown = f.bound(owner, company, version).await;
    let mut runs = Vec::new();
    // A start key names one start in the company, so each binding's start has its own.
    for (binding, key) in [(hidden, "hidden"), (shown, "shown")] {
        let (status, started) = f.start(owner, company, binding, key, json!({})).await;
        assert_eq!(status, StatusCode::CREATED, "{started}");
        runs.push(uuid(&started["run_id"]));
    }
    let listed = async |user: Uuid| {
        let (_, bindings) = answer(
            bindings::list(
                f.state(),
                by(user),
                Path(company),
                Query(ListQuery::default()),
            )
            .await,
        )
        .await;
        let (_, runs) = answer(
            bindings::runs(
                f.state(),
                by(user),
                Path(company),
                Query(bindings::RunsQuery::default()),
            )
            .await,
        )
        .await;
        let ids = |page: &Value, key: &str| {
            Fixture::items(page)
                .0
                .iter()
                .map(|row| uuid(&row[key]))
                .collect::<Vec<_>>()
        };
        (ids(&bindings, "binding_id"), ids(&runs, "run_id"))
    };
    let (bindings_seen, runs_seen) = listed(owner).await;
    assert!(bindings_seen.contains(&hidden) && bindings_seen.contains(&shown));
    assert!(runs_seen.contains(&runs[0]) && runs_seen.contains(&runs[1]));
    let (bindings_seen, runs_seen) = listed(f.admin).await;
    assert_eq!(bindings_seen, [shown]);
    assert_eq!(runs_seen, [runs[1]]);
    // A page of one, newest first: the admin's second page scanned the hidden run and shows
    // nothing, and says there is nothing after it.
    let page = async |user: Uuid, after: Option<Uuid>| {
        let (status, page) = answer(
            bindings::runs(
                f.state(),
                by(user),
                Path(company),
                Query(bindings::RunsQuery {
                    after,
                    limit: Some(1),
                    ..Default::default()
                }),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{page}");
        Fixture::items(&page)
    };
    let (first, next) = page(f.admin, None).await;
    assert_eq!((first.len(), next), (1, Some(runs[1])));
    let (second, next) = page(f.admin, Some(runs[1])).await;
    assert!(second.is_empty() && next.is_none(), "{second:?}");
    let (second, _) = page(owner, Some(runs[1])).await;
    assert_eq!(uuid(&second[0]["run_id"]), runs[0]);
    let read = answer(bindings::read(f.state(), by(f.admin), Path((company, hidden))).await).await;
    assert_eq!(read.0, StatusCode::NOT_FOUND, "{}", read.1);
    assert_eq!(
        f.run(f.admin, company, runs[0]).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(f.run(f.admin, company, runs[1]).await.0, StatusCode::OK);
    assert_eq!(
        f.start(f.admin, company, hidden, "admin", json!({}))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}
