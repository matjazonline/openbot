use super::super::test_support::{Fixture, text};
use super::*;
use crate::application::workflow::http::{HttpConnectionWrite, HttpConnections, HttpOrigin};

#[tokio::test]
async fn autonomous_agent_authoring_catalog_connection_pages_are_scoped_bounded_and_complete() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let mut ids = Vec::new();
    for _ in 0..51 {
        ids.push(
            f.f.persistence
                .create_http(
                    f.scope().company,
                    f.scope().actor,
                    HttpConnectionWrite {
                        origin: HttpOrigin::parse("http://127.0.0.1:9").unwrap(),
                        bearer: false,
                    },
                )
                .await
                .unwrap()
                .id,
        );
    }
    ids.sort();
    let page = |after| {
        connections_json(
            f.workspace(f.f.owner.id),
            Path((f.f.company.id, f.agent.id)),
            Query(ConnectionQuery {
                kind: AgentToolKind::Http,
                after,
            }),
        )
    };
    let first: serde_json::Value =
        serde_json::from_str(&text(page(None).await.unwrap()).await).unwrap();
    assert_eq!(first["items"].as_array().unwrap().len(), 50);
    let after: Uuid = serde_json::from_value(first["next"].clone()).unwrap();
    let second: serde_json::Value =
        serde_json::from_str(&text(page(Some(after)).await.unwrap()).await).unwrap();
    assert_eq!(second["items"].as_array().unwrap().len(), 1);
    assert!(second["next"].is_null());
    let found = first["items"]
        .as_array()
        .unwrap()
        .iter()
        .chain(second["items"].as_array().unwrap())
        .map(|row| serde_json::from_value::<Uuid>(row["id"].clone()).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(found, ids);
    let html = catalog_form(
        f.workspace(f.f.owner.id),
        Path(f.agent.id),
        Query(CatalogQuery {
            company_id: f.f.company.id,
            kind: AgentToolKind::Http,
            connection: None,
            after_connection: None,
            after_operation: None,
        }),
    )
    .await
    .unwrap()
    .0;
    assert!(html.contains("More connections") && !html.contains("authoring-secret-never-return"));
    for user in [f.f.member.id, f.f.stranger.id] {
        assert!(matches!(
            connections_json(
                f.workspace(user),
                Path((f.f.company.id, f.agent.id)),
                Query(ConnectionQuery {
                    kind: AgentToolKind::Http,
                    after: None
                })
            )
            .await,
            Err(AppError::NotFound(_))
        ));
    }
}

#[tokio::test]
async fn autonomous_agent_authoring_catalog_builtin_choices_do_not_offer_builder_or_slots() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let output = text(
        options_json(
            f.workspace(f.f.owner.id),
            Path((f.f.company.id, f.agent.id)),
            Query(OptionsQuery {
                kind: AgentToolKind::Builtin,
                connection: None,
                after: None,
            }),
        )
        .await
        .unwrap(),
    )
    .await;
    let json: serde_json::Value = serde_json::from_str(&output).unwrap();
    let mut names: Vec<_> = json["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["name"].as_str().unwrap())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "message.reply",
            "message.send",
            "read_escalation",
            "respond_to_escalation",
            "search_knowledge"
        ]
    );
    assert!(!output.contains("list_company_agents") && !output.contains("create_agent_channel"));
    assert!(
        !output.contains("private immutable instruction marker")
            && !output.contains("authoring-secret-never-return")
    );
    assert!(matches!(
        history_json(
            f.workspace(f.f.owner.id),
            Path((f.f.company.id, f.agent.id)),
            Query(HistoryQuery {
                before_created_at: Some(chrono::Utc::now()),
                before_id: None
            })
        )
        .await,
        Err(AppError::BadRequest(_))
    ));
    assert!(matches!(
        options_json(
            f.workspace(f.f.stranger.id),
            Path((f.f.foreign.id, f.agent.id)),
            Query(OptionsQuery {
                kind: AgentToolKind::Builtin,
                connection: None,
                after: None
            })
        )
        .await,
        Err(AppError::NotFound(_))
    ));
}
