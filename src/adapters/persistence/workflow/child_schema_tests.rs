//! Schema guards for runtime children and rerun links (07-D1 §D1.3). Raw SQL on purpose: these
//! constraints hold whatever writes the rows.
use super::*;

fn data_map() -> Value {
    serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap()
}

async fn code(result: Result<sqlx::postgres::PgQueryResult, sqlx::Error>) -> String {
    result
        .unwrap_err()
        .as_database_error()
        .unwrap()
        .code()
        .unwrap()
        .into_owned()
}

/// The SQLSTATE of a statement, raised at once or at commit by a deferred constraint.
async fn commit_code(
    mut tx: sqlx::Transaction<'_, sqlx::Postgres>,
    sql: &str,
    binds: &[Uuid],
) -> String {
    let mut query = sqlx::query(sql);
    for bind in binds {
        query = query.bind(*bind);
    }
    let error = match query.execute(&mut *tx).await {
        Err(error) => error,
        Ok(_) => tx.commit().await.unwrap_err(),
    };
    error
        .as_database_error()
        .unwrap()
        .code()
        .unwrap()
        .into_owned()
}

/// A top-level run of `f`'s own binding.
async fn admitted_root(f: &AdmissionFixture) -> RunId {
    let command = f.prepare(f.request("original", f.manual())).await;
    f.persistence().admit(&command).await.unwrap();
    command.proposed_run_id()
}

#[tokio::test]
async fn workflow_child_schema_requires_the_parent_wait_of_the_same_execution() {
    let (f, scopes) = AdmissionFixture::chain(data_map(), 1).await;
    let (parent, child) = (scopes[0], scopes[1]);
    let pool = f.persistence().pool();
    // A child run written without its parent's child_run wait fails at commit.
    let orphan = Uuid::new_v4();
    let tx = pool.begin().await.unwrap();
    assert_eq!(
        commit_code(tx, "INSERT INTO workflow_runs SELECT (jsonb_populate_record(NULL::workflow_runs,to_jsonb(run)||jsonb_build_object('id',$2::uuid,'terminal_wakeup_sequence',NULL,'state','queued','revision',1))).* FROM workflow_runs AS run WHERE run.id=$1", &[child.run.as_uuid(), orphan]).await,
        "23514"
    );
    // A wait cannot name a child of another execution of its run: a fresh child of the parent
    // execution, waited on from a second execution, breaks the lineage key (checked at once).
    let second = Uuid::new_v4();
    let fresh = Uuid::new_v4();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS workflow_wait_child_link IMMEDIATE")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO workflow_executions(company_id,run_id,id,step_id,activation) VALUES($1,$2,$3,'start',2)")
        .bind(parent.company.as_uuid()).bind(parent.run.as_uuid()).bind(second).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO workflow_runs SELECT (jsonb_populate_record(NULL::workflow_runs,to_jsonb(run)||jsonb_build_object('id',$2::uuid,'terminal_wakeup_sequence',NULL,'state','queued','revision',1))).* FROM workflow_runs AS run WHERE run.id=$1")
        .bind(child.run.as_uuid()).bind(fresh).execute(&mut *tx).await.unwrap();
    assert_eq!(
        code(sqlx::query("INSERT INTO workflow_waits(company_id,run_id,execution_id,id,reason,deadline,child_run_id,child_ordinal) SELECT company_id,run_id,$2,$3,'child_run',deadline,$4,1 FROM workflow_waits WHERE child_run_id=$1")
            .bind(child.run.as_uuid()).bind(second).bind(Uuid::new_v4()).bind(fresh).execute(&mut *tx).await).await,
        "23503"
    );
    drop(tx);
    // Shape: a child_run wait carries both child columns, other waits neither; ordinals bounded.
    for sql in [
        "INSERT INTO workflow_waits(company_id,run_id,execution_id,id,reason,deadline,child_run_id) SELECT company_id,run_id,execution_id,$2,'child_run',deadline,child_run_id FROM workflow_waits WHERE child_run_id=$1",
        "INSERT INTO workflow_waits(company_id,run_id,execution_id,id,reason,deadline,child_run_id,child_ordinal) SELECT company_id,run_id,execution_id,$2,'timer',deadline,child_run_id,child_ordinal FROM workflow_waits WHERE child_run_id=$1",
        "INSERT INTO workflow_waits(company_id,run_id,execution_id,id,reason,deadline,child_run_id,child_ordinal) SELECT company_id,run_id,execution_id,$2,'child_run',deadline,child_run_id,1001 FROM workflow_waits WHERE child_run_id=$1",
    ] {
        assert_eq!(
            code(
                sqlx::query(sql)
                    .bind(child.run.as_uuid())
                    .bind(Uuid::new_v4())
                    .execute(pool)
                    .await
            )
            .await,
            "23514",
            "{sql}"
        );
    }
    // Nor can it name a real child of another company, linked under that company's parent.
    let (_, theirs) = AdmissionFixture::chain_in(
        Fixture::in_database(f.binding.fixture._db.clone(), "-foreign").await,
        data_map(),
        1,
    )
    .await;
    let tx = pool.begin().await.unwrap();
    assert_eq!(
        commit_code(tx, "INSERT INTO workflow_waits(company_id,run_id,execution_id,id,reason,deadline,child_run_id,child_ordinal,state,settled_at) SELECT company_id,run_id,execution_id,$2,'child_run',deadline,$3,2,'cancelled',clock_timestamp() FROM workflow_waits WHERE child_run_id=$1", &[child.run.as_uuid(), Uuid::new_v4(), theirs[1].run.as_uuid()]).await,
        "23503"
    );
    // One wait per child, and one wait per (execution, ordinal).
    sqlx::query("UPDATE workflow_waits SET state='completed',settled_at=clock_timestamp() WHERE child_run_id=$1")
        .bind(child.run.as_uuid()).execute(pool).await.unwrap();
    assert_eq!(
        code(sqlx::query("INSERT INTO workflow_waits(company_id,run_id,execution_id,id,reason,deadline,child_run_id,child_ordinal) SELECT company_id,run_id,execution_id,$2,'child_run',deadline,child_run_id,2 FROM workflow_waits WHERE child_run_id=$1")
            .bind(child.run.as_uuid()).bind(Uuid::new_v4()).execute(pool).await).await,
        "23505"
    );
    let ordinals: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_waits WHERE run_id=$1 AND execution_id=$2 AND reason='child_run'")
        .bind(parent.run.as_uuid()).bind(parent.execution.as_uuid()).fetch_one(pool).await.unwrap();
    assert_eq!(ordinals, 1);
}

#[tokio::test]
async fn workflow_rerun_schema_links_only_a_top_level_run_of_the_same_workflow_and_tenant() {
    let (f, scopes) = AdmissionFixture::chain(data_map(), 1).await;
    let (root, child) = (scopes[0], scopes[1]);
    let pool = f.persistence().pool();
    let copy = "INSERT INTO workflow_runs SELECT (jsonb_populate_record(NULL::workflow_runs,to_jsonb(run)||jsonb_build_object('id',$2::uuid,'terminal_wakeup_sequence',NULL,'state','queued','waiting_reason',NULL,'revision',1,'rerun_of_run_id',$3::uuid))).* FROM workflow_runs AS run WHERE run.id=$1";
    // A child original is refused; so is a run that would be both child and rerun.
    assert_eq!(
        code(
            sqlx::query(copy)
                .bind(root.run.as_uuid())
                .bind(Uuid::new_v4())
                .bind(child.run.as_uuid())
                .execute(pool)
                .await
        )
        .await,
        "23514"
    );
    let tx = pool.begin().await.unwrap();
    assert_eq!(
        commit_code(
            tx,
            &copy.replace("$3::uuid", "$1::uuid"),
            &[child.run.as_uuid(), Uuid::new_v4()]
        )
        .await,
        "23514"
    );
    // An unknown original, a real top-level run of another workflow of this company, and a real
    // top-level run of another company have no same-company, same-workflow row.
    let sibling = f.sibling(WorkflowId::new(Uuid::from_u128(0x0730))).await;
    let foreign = f.foreign().await;
    let mut originals = vec![Uuid::new_v4()];
    for other in [&sibling, &foreign] {
        originals.push(admitted_root(other).await.as_uuid());
    }
    for original in originals {
        assert_eq!(
            code(
                sqlx::query(copy)
                    .bind(root.run.as_uuid())
                    .bind(Uuid::new_v4())
                    .bind(original)
                    .execute(pool)
                    .await
            )
            .await,
            "23503",
            "{original}"
        );
    }
    // A real top-level original links, and the link is immutable.
    let rerun = Uuid::new_v4();
    sqlx::query(copy)
        .bind(root.run.as_uuid())
        .bind(rerun)
        .bind(root.run.as_uuid())
        .execute(pool)
        .await
        .unwrap();
    assert_eq!(
        code(
            sqlx::query("UPDATE workflow_runs SET rerun_of_run_id=NULL WHERE id=$1")
                .bind(rerun)
                .execute(pool)
                .await
        )
        .await,
        "23514"
    );
}
