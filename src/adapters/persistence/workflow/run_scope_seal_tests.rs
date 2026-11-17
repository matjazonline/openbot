use super::*;

async fn unsealed(f: &RoleFixture, key: &str) -> RunId {
    super::super::super::storage_tests::autonomous_agent_storage_tests::native_runs::unsealed_root(
        &f.admission,
        key,
    )
    .await
    .run
}

async fn read_unit(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    f: &RoleFixture,
    run: RunId,
    unit: DepartmentId,
) {
    // Deliberately do not tenant-filter the selected owner: the fresh seal must
    // reject an inserted foreign owner, rather than a fixture hiding that row.
    let result = sqlx::query(
        "INSERT INTO workflow_run_scopes(company_id,run_id,unit_id,access,unit_slug,ancestor)
         SELECT $1,$2,id,'read',slug::text,true FROM departments WHERE id=$3",
    )
    .bind(f.company().as_uuid())
    .bind(run.as_uuid())
    .bind(unit.as_uuid())
    .execute(&mut **tx)
    .await
    .unwrap();
    assert_eq!(result.rows_affected(), 1);
}

pub(super) async fn root(f: &RoleFixture) -> DepartmentId {
    crate::adapters::persistence::company::root_on(
        &mut f.p().pool().acquire().await.unwrap(),
        f.company(),
    )
    .await
    .unwrap()
}

async fn authority_rows(f: &RoleFixture, run: RunId) -> i64 {
    sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM workflow_run_scopes WHERE company_id=$1 AND run_id=$2)
          +(SELECT count(*) FROM workflow_run_scope_snapshots WHERE company_id=$1 AND run_id=$2)",
    )
    .bind(f.company().as_uuid())
    .bind(run.as_uuid())
    .fetch_one(f.p().pool())
    .await
    .unwrap()
}

async fn refuse_seal(mut tx: sqlx::Transaction<'_, sqlx::Postgres>, f: &RoleFixture, run: RunId) {
    let error = sqlx::query(
        "INSERT INTO workflow_run_scope_snapshots(company_id,run_id,state) VALUES($1,$2,'available')",
    )
    .bind(f.company().as_uuid())
    .bind(run.as_uuid())
    .execute(&mut *tx)
    .await
    .unwrap_err();
    let error = error.as_database_error().unwrap();
    assert_eq!(error.code().as_deref(), Some("23514"));
    assert_eq!(
        error.constraint(),
        Some("workflow_guard:guard_workflow_scope_seal")
    );
    tx.rollback().await.unwrap();
    assert_eq!(authority_rows(f, run).await, 0);
}

#[tokio::test]
async fn memory_scope_admission_fresh_seal_rejects_foreign_and_ineligible_live_owners() {
    let f = RoleFixture::new().await;
    let foreign = foreign_root(&f).await;
    let local_root = root(&f).await;
    let run = unsealed(&f, "fresh-foreign").await;
    let mut tx = f.p().pool().begin().await.unwrap();
    read_unit(&mut tx, &f, run, local_root).await;
    read_unit(&mut tx, &f, run, foreign).await;
    refuse_seal(tx, &f, run).await;

    let run = unsealed(&f, "fresh-ineligible").await;
    let mut tx = f.p().pool().begin().await.unwrap();
    read_unit(&mut tx, &f, run, root(&f).await).await;
    let external = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO principals(id,company_id,kind,display_label) VALUES($1,$2,'external','External')",
    )
    .bind(external)
    .bind(f.company().as_uuid())
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO workflow_run_scopes(company_id,run_id,acting_principal_id,principal_id,access)
         VALUES($1,$2,$3,$3,'personal')",
    )
    .bind(f.company().as_uuid())
    .bind(run.as_uuid())
    .bind(external)
    .execute(&mut *tx)
    .await
    .unwrap();
    refuse_seal(tx, &f, run).await;
}

#[tokio::test]
async fn memory_scope_admission_fresh_seal_rejects_missing_root_and_unpaired_local_write() {
    let f = RoleFixture::new().await;
    let run = unsealed(&f, "fresh-missing-root").await;
    let mut tx = f.p().pool().begin().await.unwrap();
    read_unit(&mut tx, &f, run, f.department).await;
    refuse_seal(tx, &f, run).await;

    let run = unsealed(&f, "fresh-unpaired-write").await;
    let mut tx = f.p().pool().begin().await.unwrap();
    read_unit(&mut tx, &f, run, root(&f).await).await;
    sqlx::query(
        "INSERT INTO workflow_run_scopes(company_id,run_id,unit_id,access,unit_slug,own,chain_depth)
         SELECT $1,$2,id,'write',slug::text,true,0 FROM departments WHERE id=$3",
    )
    .bind(f.company().as_uuid())
    .bind(run.as_uuid())
    .bind(f.department.as_uuid())
    .execute(&mut *tx)
    .await
    .unwrap();
    refuse_seal(tx, &f, run).await;
}

#[tokio::test]
async fn memory_scope_admission_valid_looking_rows_cannot_commit_without_the_deferred_seal() {
    let f = RoleFixture::new().await;
    let run = unsealed(&f, "fresh-unsealed-commit").await;
    let mut tx = f.p().pool().begin().await.unwrap();
    read_unit(&mut tx, &f, run, root(&f).await).await;
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_run_scopes WHERE company_id=$1 AND run_id=$2",
    )
    .bind(f.company().as_uuid())
    .bind(run.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(
        count, 1,
        "the valid-looking row reached the deferred boundary"
    );
    let error = tx.commit().await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23503")
    );
    assert_eq!(authority_rows(&f, run).await, 0);
}

async fn foreign_root(f: &RoleFixture) -> DepartmentId {
    use crate::application::use_cases::company::{CompanyPersistence, CompanyWrite};
    // Both companies must exist in this test's owned database, so insertion of
    // the foreign scope row succeeds before the fresh seal rejects its owner.
    let company = CompanyPersistence::create(
        f.p(),
        f.admission.binding.target.actor.user_id(),
        CompanyWrite {
            name: "Foreign scope owner".into(),
            slug: format!("foreign-scope-{}", Uuid::new_v4().simple()),
            avatar_url: None,
            channel_defaults: Default::default(),
        },
    )
    .await
    .unwrap();
    crate::adapters::persistence::company::root_on(
        &mut f.p().pool().acquire().await.unwrap(),
        CompanyId::new(company.id),
    )
    .await
    .unwrap()
}
