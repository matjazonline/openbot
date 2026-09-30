//! Strict catalog comparison except two planner estimates, with complete raw artifacts.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const SUMMARY_LIMIT: usize = 20;

fn comparison_projection(catalog: &Value) -> Value {
    let mut projected = catalog.clone();
    let Some(relations) = projected.get_mut("relations").and_then(Value::as_array_mut) else {
        return projected;
    };
    for relation in relations {
        for field in ["relpages", "reltuples"] {
            // Canonicalize existing values only: missing keys and every other field stay exact.
            if let Some(value) = relation.get_mut(field) {
                *value = Value::Null;
            }
        }
    }
    projected
}

fn catalog_equal(before: &Value, after: &Value) -> bool {
    comparison_projection(before) == comparison_projection(after)
}

#[test]
fn workflow_upgrade_catalog_projection_preserves_exception_boundary() {
    let before = json!({"relations":[{"oid":1,"relpages":0,"reltuples":-1,"relfilenode":1}],
        "functions":[{"oid":2,"relpages":0}]});
    let mut estimates = before.clone();
    estimates["relations"][0]["relpages"] = json!(1);
    estimates["relations"][0]["reltuples"] = json!(100);
    assert!(catalog_equal(&before, &estimates));
    assert_eq!(
        before["relations"][0]["relpages"], 0,
        "raw input is preserved"
    );

    let mut added = estimates.clone();
    added["relations"]
        .as_array_mut()
        .unwrap()
        .push(json!({"oid":3}));
    assert!(
        !catalog_equal(&before, &added),
        "added object remains strict"
    );
    let mut removed = estimates.clone();
    removed["relations"].as_array_mut().unwrap().clear();
    assert!(
        !catalog_equal(&before, &removed),
        "removed object remains strict"
    );
    let mut missing_field = estimates.clone();
    missing_field["relations"][0]
        .as_object_mut()
        .unwrap()
        .remove("relpages");
    assert!(
        !catalog_equal(&before, &missing_field),
        "exempt field presence remains strict"
    );
    let mut storage = estimates.clone();
    storage["relations"][0]["relfilenode"] = json!(2);
    assert!(
        !catalog_equal(&before, &storage),
        "unreviewed field remains strict alongside estimates"
    );
    let mut other_group = estimates;
    other_group["functions"][0]["relpages"] = json!(1);
    assert!(
        !catalog_equal(&before, &other_group),
        "same key in another group remains strict"
    );
}

pub(super) async fn context(pool: &sqlx::PgPool) -> Value {
    sqlx::query("SELECT pg_stat_clear_snapshot()")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query_scalar(r#"SELECT jsonb_build_object(
        'database',current_database(),'server_address',inet_server_addr(),'server_port',inet_server_port(),
        'server_version_num',current_setting('server_version_num'),
        'maintenance',(SELECT jsonb_agg(to_jsonb(stats) ORDER BY stats.relid) FROM pg_stat_user_tables AS stats),
        'backends',(SELECT jsonb_agg(jsonb_build_object('pid',activity.pid,'backend_type',activity.backend_type,
            'state',activity.state,'wait_event_type',activity.wait_event_type,'wait_event',activity.wait_event))
            FROM pg_stat_activity AS activity WHERE activity.datname=current_database() AND activity.pid<>pg_backend_pid()))"#)
        .fetch_one(pool).await.unwrap()
}

fn key(group: &str, row: &Value) -> String {
    if group == "attributes" {
        format!("{}:{}", row["attrelid"], row["attnum"])
    } else {
        row["oid"].to_string()
    }
}

fn rows<'a>(group: &str, value: &'a Value) -> BTreeMap<String, &'a Value> {
    let Some(rows) = value.as_array() else {
        assert!(value.is_null(), "catalog group {group} is an array or null");
        return BTreeMap::new();
    };
    let mut indexed = BTreeMap::new();
    for row in rows {
        let key = key(group, row);
        assert!(indexed.insert(key, row).is_none(), "unique catalog keys");
    }
    indexed
}

fn bounded(value: Option<&Value>) -> Value {
    let Some(value) = value else {
        return json!({"missing":true});
    };
    let bytes = serde_json::to_vec(value).unwrap();
    if bytes.len() <= 160 {
        return value.clone();
    }
    json!({"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes)),
        "prefix":String::from_utf8_lossy(&bytes).chars().take(80).collect::<String>()})
}

fn object_delta(group: &str, key: &str, before: &Value, after: &Value) -> Vec<Value> {
    let before = before.as_object().expect("catalog object");
    let after = after.as_object().expect("catalog object");
    let fields: BTreeSet<_> = before.keys().chain(after.keys()).collect();
    fields
        .into_iter()
        .filter(|field| before.get(*field) != after.get(*field))
        .map(|field| {
            json!({"group":group,"key":key,
            "name":before.get("relname").or_else(||before.get("proname")),"field":field,
            "before":bounded(before.get(field)),"after":bounded(after.get(field))})
        })
        .collect()
}

fn delta(before: &Value, after: &Value) -> Value {
    let before = before.as_object().expect("catalog groups");
    let after = after.as_object().expect("catalog groups");
    let groups: BTreeSet<_> = before.keys().chain(after.keys()).collect();
    let mut changes = Vec::new();
    let mut objects_changed = 0;
    for group in groups {
        let old = rows(group, before.get(group).unwrap_or(&Value::Null));
        let new = rows(group, after.get(group).unwrap_or(&Value::Null));
        let keys: BTreeSet<_> = old.keys().chain(new.keys()).collect();
        for key in keys {
            if old.get(key) == new.get(key) {
                continue;
            }
            objects_changed += 1;
            match (old.get(key), new.get(key)) {
                (Some(old), Some(new)) => changes.extend(object_delta(group, key, old, new)),
                (old, new) => changes.push(json!({"group":group,"key":key,"field":"object",
                    "before":bounded(old.copied()),"after":bounded(new.copied())})),
            }
        }
    }
    json!({"objects_changed":objects_changed,"fields_changed":changes.len(),
        "truncated":changes.len()>SUMMARY_LIMIT,"changes":changes.into_iter().take(SUMMARY_LIMIT).collect::<Vec<_>>()})
}

fn write_artifacts(
    before: &Value,
    after: &Value,
    metadata: &Value,
) -> std::io::Result<std::path::PathBuf> {
    let directory =
        std::env::temp_dir().join(format!("workflow-catalog-failure-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory)?;
    let mut manifest = metadata.clone();
    for (name, value) in [("before", before), ("after", after)] {
        let bytes = serde_json::to_vec(value)?;
        manifest[name] =
            json!({"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes))});
        std::fs::write(directory.join(format!("{name}.json")), bytes)?;
    }
    std::fs::write(
        directory.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(directory)
}

pub(super) fn assert_unchanged(
    before: &Value,
    after: &Value,
    context: &Value,
    after_context: &Value,
    attempt: usize,
) {
    if before == after {
        return;
    }
    let projected_equal = catalog_equal(before, after);
    let summary = delta(before, after);
    let metadata = json!({"attempt":attempt,"before_context":context,"after_context":after_context,
        "projected_equal":projected_equal,"summary":summary});
    let artifacts = write_artifacts(before, after, &metadata)
        .expect("every raw catalog difference requires complete artifacts");
    println!(
        "catalog attempt {attempt}: projected_equal={projected_equal}; {summary}; artifacts={}",
        artifacts.display()
    );
    assert!(
        projected_equal,
        "no partial schema mutation: {summary}; full catalog artifacts: {}",
        artifacts.display()
    );
}

async fn control_rows(executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>) -> Value {
    sqlx::query_scalar(
        "SELECT jsonb_agg(to_jsonb(item) ORDER BY item.id) FROM catalog_control AS item",
    )
    .fetch_one(executor)
    .await
    .unwrap()
}

fn record_control(kind: &str, before: &Value, after: &Value, context: &Value) {
    assert!(
        before != after,
        "{kind} must be visible to raw catalog equality"
    );
    let summary = delta(before, after);
    let metadata = json!({"control":kind,"context":context,
        "projected_equal":catalog_equal(before, after),"summary":summary});
    let directory = write_artifacts(before, after, &metadata).expect("complete control artifacts");
    println!(
        "catalog control {kind}: {summary}; artifacts={}",
        directory.display()
    );
}

#[tokio::test]
async fn workflow_upgrade_catalog_maintenance_and_schema_controls() {
    let migrations = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(Vec::new()),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    let database =
        crate::adapters::persistence::test_support::own_database_with_migrations(&migrations)
            .await
            .expect("disposable catalog-control database");
    let pool = &database.pool;
    sqlx::raw_sql("CREATE TABLE catalog_control(id integer); INSERT INTO catalog_control SELECT generate_series(1,100); CREATE FUNCTION catalog_control_value() RETURNS integer LANGUAGE sql AS 'SELECT 1'")
        .execute(pool).await.unwrap();
    let rows = control_rows(pool).await;
    let before = super::schema_catalog(pool).await;
    let before_context = context(pool).await;
    sqlx::query("ANALYZE catalog_control")
        .execute(pool)
        .await
        .unwrap();
    let analyzed = super::schema_catalog(pool).await;
    assert_eq!(
        rows,
        control_rows(pool).await,
        "ANALYZE preserves every control row"
    );
    let after_context = context(pool).await;
    record_control("analyze", &before, &analyzed, &after_context);
    let relation = |catalog: &Value| {
        catalog["relations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["relname"] == "catalog_control")
            .unwrap()
            .clone()
    };
    let old = relation(&before);
    let new = relation(&analyzed);
    assert_eq!(old["relpages"].as_f64(), Some(0.0));
    assert_eq!(new["relpages"].as_f64(), Some(1.0));
    assert_eq!(old["reltuples"].as_f64(), Some(-1.0));
    assert_eq!(new["reltuples"].as_f64(), Some(100.0));
    assert!(
        catalog_equal(&before, &analyzed),
        "ANALYZE changes only the accepted estimates"
    );
    assert_unchanged(&before, &analyzed, &before_context, &after_context, 0);
    // Box the control seams so the test does not accumulate their SQLx future frames.
    Box::pin(column_control(pool, &analyzed, &rows)).await;
    Box::pin(function_control(pool, &analyzed, &rows)).await;
}

async fn column_control(pool: &sqlx::PgPool, analyzed: &Value, rows: &Value) {
    let before_context = context(pool).await;
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("ALTER TABLE catalog_control ADD COLUMN detail text")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let column = super::schema_catalog(&mut *transaction).await;
    record_control("add-column", analyzed, &column, &context(pool).await);
    assert!(
        !catalog_equal(analyzed, &column),
        "new column must fail the upgrade comparison"
    );
    assert!(
        column["attributes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|attribute| attribute["attname"] == "detail")
    );
    transaction.rollback().await.unwrap();
    let rolled_back = super::schema_catalog(pool).await;
    assert_unchanged(
        analyzed,
        &rolled_back,
        &before_context,
        &context(pool).await,
        0,
    );
    assert!(
        analyzed == &rolled_back,
        "column rollback restores the entire raw catalog"
    );
    assert_eq!(
        *rows,
        control_rows(pool).await,
        "column rollback preserves every control row"
    );
}

async fn function_control(pool: &sqlx::PgPool, analyzed: &Value, rows: &Value) {
    let before_context = context(pool).await;
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("CREATE OR REPLACE FUNCTION catalog_control_value() RETURNS integer LANGUAGE sql AS 'SELECT 2'")
        .execute(&mut *transaction).await.unwrap();
    let body = super::schema_catalog(&mut *transaction).await;
    record_control("function-body", analyzed, &body, &context(pool).await);
    assert!(
        !catalog_equal(analyzed, &body),
        "function body must fail the upgrade comparison"
    );
    let function = |catalog: &Value| {
        catalog["functions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["proname"] == "catalog_control_value")
            .unwrap()
            .clone()
    };
    let old = function(analyzed);
    let new = function(&body);
    assert_eq!(
        old["oid"], new["oid"],
        "body replacement keeps object identity"
    );
    assert_eq!(old["prosrc"], "SELECT 1");
    assert_eq!(new["prosrc"], "SELECT 2");
    transaction.rollback().await.unwrap();
    let rolled_back = super::schema_catalog(pool).await;
    assert_unchanged(
        analyzed,
        &rolled_back,
        &before_context,
        &context(pool).await,
        0,
    );
    assert!(
        analyzed == &rolled_back,
        "function rollback restores the entire raw catalog"
    );
    assert_eq!(
        *rows,
        control_rows(pool).await,
        "function rollback preserves every control row"
    );
}
