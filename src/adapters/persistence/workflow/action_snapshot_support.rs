//! Whole-database snapshots for action tests: every public relation from one repeatable-read
//! read, in a form exact enough to assert that an operation wrote nothing or exactly one fact.
use super::*;

// Catalog-owned identifiers only; one repeatable-read snapshot covers every fact,
// including reconciliation joins, witnesses and provider effects omitted by old helpers.
// One statement reads every relation: the same rows in the same order as reading them one by
// one (`per_relation_tables`), without ~120 round trips that let a parallel suite outlive
// the short fixture leases these snapshots sit between.
pub(super) async fn all_tables(f: &AdmissionFixture) -> Value {
    snapshot(f, None, &[]).await
}

/// Exactly these relations, in the form and from the one snapshot `all_tables` would report.
pub(super) async fn some_tables(f: &AdmissionFixture, tables: &[&str]) -> Value {
    snapshot(f, Some(tables), &[]).await
}

/// `all_tables` (or `some_tables`), except that each `digested` relation is reported as its row
/// count and the SHA-256 of exactly the canonical text the full form sorts and compares. Two
/// such snapshots are equal precisely when those relations hold the same rows, at a fraction of
/// the cost for large append-only history. Compare only like with like.
pub(super) async fn digested_tables(
    f: &AdmissionFixture,
    only: Option<&[&str]>,
    digested: &[&str],
) -> Value {
    snapshot(f, only, digested).await
}

/// Every public relation as its `digested_tables` row count and SHA-256: a compact, faithful
/// fingerprint of the whole database for intervals too short for full rows.
pub(super) async fn fingerprint(f: &AdmissionFixture) -> Value {
    let mut tx = read_snapshot(f).await;
    let tables = public_tables(&mut tx).await;
    let fingerprint = relations_on(&mut tx, &tables, |_| true).await;
    tx.commit().await.unwrap();
    fingerprint
}

/// `all_tables` and `fingerprint` from one repeatable-read snapshot, so the full rows are
/// exactly the state the fingerprint identifies.
pub(super) async fn tables_and_fingerprint(f: &AdmissionFixture) -> (Value, Value) {
    let mut tx = read_snapshot(f).await;
    let tables = public_tables(&mut tx).await;
    let rows = relations_on(&mut tx, &tables, |_| false).await;
    let fingerprint = relations_on(&mut tx, &tables, |_| true).await;
    tx.commit().await.unwrap();
    (rows, fingerprint)
}

async fn snapshot(f: &AdmissionFixture, only: Option<&[&str]>, digested: &[&str]) -> Value {
    let mut tx = read_snapshot(f).await;
    let tables = match only {
        None => public_tables(&mut tx).await,
        Some(tables) => tables.iter().map(ToString::to_string).collect(),
    };
    let state = relations_on(&mut tx, &tables, |table| digested.contains(&table)).await;
    tx.commit().await.unwrap();
    state
}

async fn read_snapshot(f: &AdmissionFixture) -> sqlx::Transaction<'static, sqlx::Postgres> {
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx
}

/// One statement reading every named relation, digested where `digest` says so.
async fn relations_on(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tables: &[String],
    digest: impl Fn(&str) -> bool,
) -> Value {
    let relations = tables
        .iter()
        .enumerate()
        .map(|(ordinal, table)| {
            let rows = if digest(table) {
                relation_digest(table)
            } else {
                relation_rows(table)
            };
            format!("({ordinal},{rows})")
        })
        .collect::<Vec<_>>()
        .join(",");
    let rows = sqlx::query_scalar::<_, Value>(&format!("SELECT COALESCE(jsonb_agg(relation.rows ORDER BY relation.ordinal),'[]'::jsonb) FROM (VALUES {relations}) AS relation(ordinal,rows)"))
        .fetch_one(&mut **tx).await.unwrap();
    let Value::Array(rows) = rows else {
        panic!("one array of relations")
    };
    assert_eq!(rows.len(), tables.len());
    Value::Object(tables.iter().cloned().zip(rows).collect())
}

async fn public_tables(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT tablename::text FROM pg_tables WHERE schemaname='public' ORDER BY tablename",
    )
    .fetch_all(&mut **tx)
    .await
    .unwrap()
}

fn relation_rows(table: &str) -> String {
    let quoted = table.replace('"', "\"\"");
    format!(
        "(SELECT COALESCE(jsonb_agg(to_jsonb(record) ORDER BY to_jsonb(record)::text),'[]'::jsonb) FROM public.\"{quoted}\" AS record)"
    )
}

fn relation_digest(table: &str) -> String {
    let quoted = table.replace('"', "\"\"");
    format!(
        "(SELECT jsonb_build_object('rows',count(*),'sha256',encode(sha256(convert_to(COALESCE(string_agg(to_jsonb(record)::text,E'\\n' ORDER BY to_jsonb(record)::text COLLATE \"C\"),''),'UTF8')),'hex')) FROM public.\"{quoted}\" AS record)"
    )
}

/// One relation in exactly the form `all_tables` reports it; a cheap read for short intervals.
pub(super) async fn relation(f: &AdmissionFixture, table: &str) -> Value {
    sqlx::query_scalar(&format!("SELECT {}", relation_rows(table)))
        .fetch_one(f.persistence().pool())
        .await
        .unwrap()
}

/// The one-relation-per-query read `all_tables` replaced, kept as its equivalence oracle.
pub(super) async fn per_relation_tables(f: &AdmissionFixture) -> Value {
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await
        .unwrap();
    let mut state = serde_json::Map::new();
    for table in public_tables(&mut tx).await {
        let rows: Value = sqlx::query_scalar(&format!("SELECT {}", relation_rows(&table)))
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        state.insert(table, rows);
    }
    tx.commit().await.unwrap();
    Value::Object(state)
}
