use super::*;
use crate::entities::department::DepartmentId;
use std::collections::{BTreeMap, BTreeSet};

struct DeskTools {
    tools: Tools,
    namespaces: BTreeMap<String, Uuid>,
    observations: BTreeMap<String, Uuid>,
    local: DepartmentId,
}

impl DeskTools {
    async fn new() -> Self {
        let s = Box::pin(SearchFixture::with_lead_desk_tools(
            AccessLevel::L5,
            &MEMORY,
        ))
        .await;
        let tools = Box::pin(Tools::from_search(s)).await;
        let units: Vec<(Uuid, String)> = sqlx::query_as(
            "SELECT id,slug::text FROM departments WHERE company_id=$1 ORDER BY slug",
        )
        .bind(tools.company().as_uuid())
        .fetch_all(tools.p().pool())
        .await
        .unwrap();
        let mut namespaces = BTreeMap::new();
        let mut observations = BTreeMap::new();
        let mut local = None;
        for (id, slug) in units {
            let unit = DepartmentId::new(id);
            if slug == "memory-local" {
                local = Some(unit);
            }
            let namespace = tools.ready(NamespaceOwner::Unit(unit)).await;
            let observation =
                tools.seed(namespace, 2, &format!("Selector {slug}"), "Selector fact");
            namespaces.insert(slug.clone(), namespace);
            observations.insert(slug, observation);
        }
        let personal = tools.ready(tools.agent_owner().await).await;
        tools.seed(personal, 2, "Selector personal", "Selector private fact");
        Self {
            tools,
            namespaces,
            observations,
            local: local.unwrap(),
        }
    }

    fn expected(&self, names: &[&str]) -> BTreeSet<Uuid> {
        names.iter().map(|name| self.namespaces[*name]).collect()
    }

    async fn search(&self, scopes: Value, names: &[&str]) {
        let t = &self.tools;
        let call = t
            .prepare(
                MemoryTool::Search,
                json!({"query":"selector","scopes":scopes,"limit":10}),
            )
            .await;
        let invocation = call.invocation().unwrap().0;
        let before = t.service_calls("observations/search");
        let result = t.call(call).await;
        let items = result["items"].as_array().expect("successful scope search");
        let actual: BTreeSet<Uuid> = items
            .iter()
            .map(|item| Uuid::parse_str(item["namespace"].as_str().unwrap()).unwrap())
            .collect();
        assert_eq!(actual, self.expected(names), "{result}");
        assert_eq!(
            items.len(),
            names.len(),
            "no duplicated observations: {result}"
        );
        assert_eq!(
            t.service_calls("observations/search") - before,
            names.len(),
            "exactly one query for each unique selected namespace"
        );
        for item in items {
            let namespace = Uuid::parse_str(item["namespace"].as_str().unwrap()).unwrap();
            let slug = self
                .namespaces
                .iter()
                .find(|(_, id)| **id == namespace)
                .unwrap()
                .0;
            let label = if slug == "memory-local" || !slug.contains("report") {
                format!("unit:{slug}")
            } else {
                format!("reports:{slug}")
            };
            assert_eq!(item["scope"], label);
        }
        let stored = t.committed().await;
        let proof = stored
            .consumptions
            .iter()
            .find(|row| row["invocation"] == json!(invocation))
            .unwrap();
        let proven: BTreeSet<Uuid> = proof["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| Uuid::parse_str(item["namespace"].as_str().unwrap()).unwrap())
            .collect();
        assert_eq!(
            proven, actual,
            "consumption binds the exact returned namespaces"
        );
    }
}

#[tokio::test]
async fn memory_scope_tools_unit_reports_filter_deduplicate_and_keep_exact_labels_and_references() {
    let d = Box::pin(DeskTools::new()).await;
    d.search(json!(["unit"]), &["memory-local"]).await;
    let reports = ["memory-local", "report-a", "report-b", "root-report"];
    d.search(json!(["reports"]), &reports).await;
    let root = d
        .namespaces
        .keys()
        .find(|slug| !reports.contains(&slug.as_str()))
        .unwrap();
    d.search(json!(["ancestors"]), &[root]).await;
    let mut all = reports.to_vec();
    all.push(root);
    d.search(json!(["unit", "ancestors", "reports"]), &all)
        .await;

    let t = &d.tools;
    for (selector, slug, label) in [
        ("unit", "memory-local", "unit:memory-local"),
        ("reports", "report-a", "reports:report-a"),
    ] {
        let id = d.observations[slug];
        let timeline = t
            .prepare(
                MemoryTool::Timeline,
                json!({"scopes":[selector],"id":id,"before":0,"after":0}),
            )
            .await;
        let result = t.call(timeline).await;
        assert_eq!(result["items"].as_array().unwrap().len(), 1, "{result}");
        assert_eq!(result["items"][0]["namespace"], json!(d.namespaces[slug]));
        assert_eq!(result["items"][0]["scope"], label);
        let details = t
            .prepare(
                MemoryTool::Details,
                json!({"items":[{"namespace":d.namespaces[slug],"id":id}]}),
            )
            .await;
        let result = t.call(details).await;
        assert_eq!(result["items"][0]["namespace"], json!(d.namespaces[slug]));
        assert_eq!(result["items"][0]["id"], json!(id));
        assert_eq!(result["items"][0]["scope"], label);
        assert_eq!(result["items"][0]["text"], "Selector fact");
    }
}

#[tokio::test]
async fn memory_scope_tools_default_and_explicit_unit_saves_freeze_only_the_local_target() {
    let d = Box::pin(DeskTools::new()).await;
    let t = &d.tools;
    let NamespaceOwner::Principal(personal) = t.agent_owner().await else {
        panic!("agent")
    };
    for arguments in [
        json!({"facts":["Default local fact"]}),
        json!({"scope":"unit","facts":["Explicit local fact"]}),
        json!({"scope":"personal","facts":["Private agent fact"]}),
    ] {
        let call = t.prepare(MemoryTool::Save, arguments.clone()).await;
        let invocation = call.invocation().unwrap().0;
        let result = t.call(call).await;
        assert_eq!(result["status"], "accepted", "{result}");
        let committed = t.committed().await;
        let delivery = committed
            .deliveries
            .iter()
            .find(|row| row["invocation"] == json!(invocation))
            .unwrap();
        if arguments["scope"] == "personal" {
            assert_eq!(delivery["scope"], "principal");
            assert_eq!(delivery["principal"], json!(personal.as_uuid()));
            assert!(delivery["unit"].is_null());
        } else {
            assert_eq!(delivery["scope"], "unit");
            assert_eq!(delivery["unit"], json!(d.local.as_uuid()));
            assert!(delivery["principal"].is_null());
        }
    }
    assert_eq!(t.committed().await.deliveries.len(), 3);
}

#[tokio::test]
async fn memory_scope_tools_explicit_unit_save_without_a_local_unit_creates_no_intent() {
    let t = Tools::new(AccessLevel::L5).await;
    let call = t
        .prepare(
            MemoryTool::Save,
            json!({"scope":"unit","facts":["There is no local unit"]}),
        )
        .await;
    let result = t.call(call).await;
    assert_eq!(error_code(&result), "memory.scope_unavailable", "{result}");
    assert!(t.committed().await.deliveries.is_empty());
}
