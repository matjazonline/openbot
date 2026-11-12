//! Storage/sealing checks; production role-write/park/submit integration has separate coverage.
use super::*;
use crate::adapters::persistence::workflow::people_resolutions::{self, ResolutionKey};
use crate::application::workflow::people_resolution::{
    PeopleResolution, PeopleResolutionFormat, PeopleResolutionRevision,
};
use crate::entities::agent_capability::DependencyManifestId;
use crate::entities::department::RoleRefusalKind;

#[derive(sqlx::FromRow)]
struct RootIdentity {
    manifest_id: Uuid,
    version_id: Uuid,
}

async fn document(f: &PeopleFixture, run: RunId) -> PeopleResolution {
    let root: RootIdentity = sqlx::query_as(
        "SELECT ownership.manifest_id,run.version_id FROM run_dependency_manifests AS ownership \
         JOIN workflow_runs AS run ON run.company_id=ownership.company_id AND run.id=ownership.run_id \
         WHERE ownership.company_id=$1 AND ownership.run_id=$2",
    ).bind(f.role.company().as_uuid()).bind(run.as_uuid()).fetch_one(f.role.p().pool()).await.unwrap();
    PeopleResolution {
        format: PeopleResolutionFormat::V1,
        company_id: f.role.company(),
        root_run_id: run,
        manifest_id: DependencyManifestId::new(root.manifest_id),
        version_id: VersionId::new(root.version_id),
        step: StepId::parse("start").unwrap(),
        role: DepartmentRole::Reviewer,
        resolution_revision: PeopleResolutionRevision::try_from(1).unwrap(),
        context_department_id: f.role.department,
        department_id: Some(f.role.department),
        principals: f.principals.clone(),
        refusal: None,
    }
}

fn key(value: &PeopleResolution) -> ResolutionKey<'_> {
    ResolutionKey {
        company: value.company_id,
        root: value.root_run_id,
        manifest: value.manifest_id,
        version: value.version_id,
        step: &value.step,
        role: value.role,
    }
}

async fn insert_fact_only(
    db: &mut PgConnection,
    value: &PeopleResolution,
) -> Result<(), sqlx::Error> {
    let content: Value = serde_json::from_slice(&value.canonical_content().unwrap()).unwrap();
    sqlx::query("INSERT INTO workflow_people_role_resolutions(company_id,root_run_id,manifest_id,version_id,step,role,resolution_revision,context_department_id,department_id,refusal,content) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)")
        .bind(value.company_id.as_uuid()).bind(value.root_run_id.as_uuid()).bind(value.manifest_id.as_uuid())
        .bind(value.version_id.as_uuid()).bind(value.step.as_str()).bind(value.role.as_str())
        .bind(i64::try_from(value.resolution_revision.get()).unwrap()).bind(value.context_department_id.as_uuid())
        .bind(value.department_id.map(|id| id.as_uuid())).bind(content["refusal"].as_str()).bind(&content)
        .execute(db).await?;
    Ok(())
}

#[tokio::test]
async fn workflow_reviewer_follow_storage_seals_later_and_empty_sets_without_changing_zero() {
    let f = PeopleFixture::new(2).await;
    let run = f.role.admitted("later-people-storage").await;
    let frozen = f.role.manifest(run).await;
    let mut value = document(&f, run).await;
    value.principals.truncate(1);
    let mut tx = f.role.p().pool().begin().await.unwrap();
    value.resolution_revision = people_resolutions::next_revision_on(&mut tx, &key(&value))
        .await
        .unwrap();
    people_resolutions::persist_on(&mut tx, &value)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let mut connection = f.role.p().pool().acquire().await.unwrap();
    let restored =
        people_resolutions::read_on(&mut connection, &key(&value), value.resolution_revision)
            .await
            .unwrap();
    assert_eq!(
        restored.content_hash().unwrap(),
        value.content_hash().unwrap()
    );
    value.principals.clear();
    value.refusal = Some(RoleRefusalKind::NoHolder);
    value.department_id = None;
    let mut tx = f.role.p().pool().begin().await.unwrap();
    value.resolution_revision = people_resolutions::next_revision_on(&mut tx, &key(&value))
        .await
        .unwrap();
    assert_eq!(value.resolution_revision.get(), 2);
    people_resolutions::persist_on(&mut tx, &value)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let empty =
        people_resolutions::read_on(&mut connection, &key(&value), value.resolution_revision)
            .await
            .unwrap();
    assert!(empty.principals.is_empty());
    assert_eq!(empty.refusal, Some(RoleRefusalKind::NoHolder));
    let counts: Value = sqlx::query_scalar("SELECT jsonb_build_object('admission',(SELECT count(*) FROM workflow_run_role_bindings WHERE company_id=$1 AND run_id=$2 AND resolution_revision=0),'later',(SELECT count(*) FROM workflow_run_role_bindings WHERE company_id=$1 AND run_id=$2 AND resolution_revision=1),'empty',(SELECT count(*) FROM workflow_run_role_bindings WHERE company_id=$1 AND run_id=$2 AND resolution_revision=2),'facts',(SELECT count(*) FROM workflow_people_role_resolutions WHERE company_id=$1 AND root_run_id=$2))")
        .bind(f.role.company().as_uuid()).bind(run.as_uuid()).fetch_one(&mut *connection).await.unwrap();
    assert_eq!(counts, json!({"admission":2,"later":1,"empty":0,"facts":2}));
    assert_eq!(f.role.manifest(run).await, frozen);
    for statement in [
        "UPDATE workflow_people_role_resolutions SET content=content||'{\"extra\":true}'::jsonb WHERE company_id=$1 AND root_run_id=$2",
        "DELETE FROM workflow_people_role_resolutions WHERE company_id=$1 AND root_run_id=$2",
        "DELETE FROM workflow_run_role_bindings WHERE company_id=$1 AND run_id=$2 AND resolution_revision=1",
    ] {
        let error = sqlx::query(statement)
            .bind(f.role.company().as_uuid())
            .bind(run.as_uuid())
            .execute(f.role.p().pool())
            .await
            .unwrap_err();
        assert_eq!(
            error
                .as_database_error()
                .and_then(|error| error.code())
                .as_deref(),
            Some("23514")
        );
    }
}

#[tokio::test]
async fn workflow_reviewer_follow_storage_refuses_missing_extra_and_foreign_members() {
    let f = PeopleFixture::new(2).await;
    let run = f.role.admitted("incomplete-people-storage").await;
    let mut value = document(&f, run).await;
    let mut tx = f.role.p().pool().begin().await.unwrap();
    insert_fact_only(&mut tx, &value).await.unwrap();
    let error = tx.commit().await.unwrap_err();
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.constraint()),
        Some("workflow_guard:check_workflow_people_members_complete")
    );
    value.principals.truncate(1);
    let mut tx = f.role.p().pool().begin().await.unwrap();
    people_resolutions::persist_on(&mut tx, &value)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let error = sqlx::query("INSERT INTO workflow_run_role_bindings(company_id,run_id,manifest_id,version_id,step,role,resolution_revision,department_id,principal_id) VALUES($1,$2,$3,$4,'start','reviewer',1,$5,$6)")
        .bind(value.company_id.as_uuid()).bind(run.as_uuid()).bind(value.manifest_id.as_uuid()).bind(value.version_id.as_uuid())
        .bind(f.role.department.as_uuid()).bind(f.principals[1].as_uuid()).execute(f.role.p().pool()).await.unwrap_err();
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.constraint()),
        Some("workflow_guard:guard_workflow_role_binding")
    );
    let foreign = PeopleFixture::new(1).await;
    value.resolution_revision = PeopleResolutionRevision::try_from(2).unwrap();
    value.principals = foreign.principals;
    let mut tx = f.role.p().pool().begin().await.unwrap();
    let error = people_resolutions::persist_on(&mut tx, &value)
        .await
        .unwrap_err();
    assert!(format!("{error:?}").contains("guard_workflow_role_binding"));
    tx.rollback().await.unwrap();
    let retained: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_people_role_resolutions WHERE company_id=$1 AND root_run_id=$2")
        .bind(value.company_id.as_uuid()).bind(run.as_uuid()).fetch_one(f.role.p().pool()).await.unwrap();
    assert_eq!(retained, 1);
}

#[tokio::test]
async fn workflow_reviewer_follow_storage_competing_allocators_commit_distinct_revisions() {
    let f = PeopleFixture::new(1).await;
    let run = f.role.admitted("competing-people-storage").await;
    let value = document(&f, run).await;
    let mut blocker = f.role.p().pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(value.company_id.as_uuid())
        .execute(&mut *blocker)
        .await
        .unwrap();
    let mut jobs = Vec::new();
    let mut pids = Vec::new();
    for _ in 0..2 {
        let pool = f.role.p().pool().clone();
        let mut value = value.clone();
        let (send, receive) = tokio::sync::oneshot::channel();
        jobs.push(tokio::spawn(async move {
            let mut tx = pool.begin().await.unwrap();
            let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut *tx)
                .await
                .unwrap();
            send.send(pid).unwrap();
            value.resolution_revision = people_resolutions::next_revision_on(&mut tx, &key(&value))
                .await
                .unwrap();
            people_resolutions::persist_on(&mut tx, &value)
                .await
                .unwrap();
            tx.commit().await.unwrap();
            value.resolution_revision.get()
        }));
        pids.push(receive.await.unwrap());
    }
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let waiting:i64=sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE pid=ANY($1) AND wait_event_type='Lock'")
                .bind(&pids).fetch_one(f.role.p().pool()).await.unwrap();
            if waiting==2 { break; }
            tokio::task::yield_now().await;
        }
    }).await.expect("both real allocators must be observed competing for the company lock");
    blocker.commit().await.unwrap();
    let mut revisions = Vec::new();
    for job in jobs {
        revisions.push(job.await.unwrap());
    }
    revisions.sort_unstable();
    assert_eq!(revisions, vec![1, 2]);
    let retained:i64=sqlx::query_scalar("SELECT count(*) FROM workflow_people_role_resolutions WHERE company_id=$1 AND root_run_id=$2")
        .bind(value.company_id.as_uuid()).bind(run.as_uuid()).fetch_one(f.role.p().pool()).await.unwrap();
    assert_eq!(retained, 2);
}

#[tokio::test]
async fn workflow_reviewer_follow_storage_revision_exhaustion_is_a_named_refusal() {
    let f = PeopleFixture::new(1).await;
    let run = f.role.admitted("people-storage-exhaustion").await;
    let mut value = document(&f, run).await;
    value.resolution_revision = PeopleResolutionRevision::try_from(i64::MAX as u64).unwrap();
    let mut tx = f.role.p().pool().begin().await.unwrap();
    people_resolutions::persist_on(&mut tx, &value)
        .await
        .unwrap();
    let error = people_resolutions::next_revision_on(&mut tx, &key(&value))
        .await
        .unwrap_err();
    assert!(
        matches!(error, AppError::Conflict(message) if message.contains("workflow.role_resolution_limit"))
    );
    tx.rollback().await.unwrap();
    let retained: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_people_role_resolutions WHERE company_id=$1 AND root_run_id=$2",
    )
    .bind(value.company_id.as_uuid())
    .bind(run.as_uuid())
    .fetch_one(f.role.p().pool())
    .await
    .unwrap();
    assert_eq!(retained, 0);
}
