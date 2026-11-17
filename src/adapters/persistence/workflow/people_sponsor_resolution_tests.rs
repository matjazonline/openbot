use super::*;
use crate::application::ports::department::DepartmentRoleAssignment;

async fn sponsor_document() -> (PeopleFixture, PeopleResolution) {
    let f = Box::pin(PeopleFixture::with_tracking(
        2,
        json!(["sponsor"]),
        ReviewerTracking::FollowRole,
    ))
    .await;
    let departments = crate::adapters::persistence::test_support::department_use_cases(f.role.p());
    for principal in &f.principals {
        departments
            .assign_role(
                f.role.admission.binding.target.actor.user_id(),
                f.role.company().as_uuid(),
                f.role.department,
                DepartmentRoleAssignment {
                    role: DepartmentRole::Sponsor,
                    principal: *principal,
                    decider: false,
                },
            )
            .await
            .unwrap();
    }
    let run = f.role.admitted("later-sponsor-seals").await;
    let mut value = document(&f, run).await;
    value.role = DepartmentRole::Sponsor;
    value.informed = value.principals.split_off(1);
    (f, value)
}

// Deliberately bypass Rust validation: these tests exercise the SQL trust boundary.
async fn insert_raw_fact(
    db: &mut PgConnection,
    value: &PeopleResolution,
    content: &Value,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO workflow_people_role_resolutions \
        (company_id,root_run_id,manifest_id,version_id,step,role,resolution_revision,context_department_id,department_id,refusal,content) \
        VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)")
        .bind(value.company_id.as_uuid()).bind(value.root_run_id.as_uuid()).bind(value.manifest_id.as_uuid())
        .bind(value.version_id.as_uuid()).bind(value.step.as_str()).bind(content["role"].as_str().unwrap())
        .bind(i64::try_from(value.resolution_revision.get()).unwrap()).bind(value.context_department_id.as_uuid())
        .bind(content["department_id"].as_str().map(|id| Uuid::parse_str(id).unwrap()))
        .bind(content["refusal"].as_str()).bind(content).execute(db).await?;
    Ok(())
}

async fn insert_member(
    db: &mut PgConnection,
    value: &PeopleResolution,
    principal: PrincipalId,
    capacity: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO workflow_run_role_bindings \
        (company_id,run_id,manifest_id,version_id,step,role,resolution_revision,department_id,principal_id,capacity) \
        VALUES($1,$2,$3,$4,$5,'sponsor',$6,$7,$8,$9)")
        .bind(value.company_id.as_uuid()).bind(value.root_run_id.as_uuid()).bind(value.manifest_id.as_uuid())
        .bind(value.version_id.as_uuid()).bind(value.step.as_str()).bind(i64::try_from(value.resolution_revision.get()).unwrap())
        .bind(value.department_id.unwrap().as_uuid()).bind(principal.as_uuid()).bind(capacity).execute(db).await?;
    Ok(())
}

fn rejected(error: &sqlx::Error, constraint: &str) {
    let error = error.as_database_error().unwrap();
    assert_eq!(error.code().as_deref(), Some("23514"));
    assert_eq!(error.constraint(), Some(constraint));
}

async fn assert_no_later_facts(f: &PeopleFixture, value: &PeopleResolution) {
    let facts: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_people_role_resolutions WHERE company_id=$1 AND root_run_id=$2")
        .bind(value.company_id.as_uuid()).bind(value.root_run_id.as_uuid()).fetch_one(f.role.p().pool()).await.unwrap();
    assert_eq!(
        facts, 0,
        "the rejected transaction leaves no later resolution"
    );
}

#[tokio::test]
async fn workflow_role_sponsor_later_people_sql_rejects_invalid_informed_documents_and_retains_controls()
 {
    let (f, mut value) = Box::pin(sponsor_document()).await;
    let frozen = f.role.manifest(value.root_run_id).await;
    for variant in ["missing", "overlap", "duplicate", "refusal", "two_deciders"] {
        let mut content = serde_json::to_value(&value).unwrap();
        match variant {
            "missing" => {
                content.as_object_mut().unwrap().remove("informed");
            }
            "overlap" => content["informed"] = content["principals"].clone(),
            "duplicate" => content["informed"] = json!([value.informed[0], value.informed[0]]),
            "refusal" => {
                content["refusal"] = json!("no_decider");
                content["department_id"] = Value::Null;
                content["principals"] = json!([]);
            }
            "two_deciders" => {
                content["principals"] = json!(f.principals);
                content["informed"] = json!([]);
            }
            _ => unreachable!(),
        }
        let mut tx = f.role.p().pool().begin().await.unwrap();
        let error = insert_raw_fact(&mut tx, &value, &content)
            .await
            .unwrap_err();
        rejected(&error, "workflow_guard:guard_workflow_people_resolution");
        tx.rollback().await.unwrap();
        assert_no_later_facts(&f, &value).await;
    }
    let mut tx = f.role.p().pool().begin().await.unwrap();
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
        restored.canonical_content().unwrap(),
        value.canonical_content().unwrap()
    );
    value.resolution_revision = PeopleResolutionRevision::try_from(2).unwrap();
    value.principals.clear();
    value.informed.clear();
    value.department_id = None;
    value.refusal = Some(RoleRefusalKind::NoDecider);
    let mut tx = f.role.p().pool().begin().await.unwrap();
    people_resolutions::persist_on(&mut tx, &value)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let restored =
        people_resolutions::read_on(&mut connection, &key(&value), value.resolution_revision)
            .await
            .unwrap();
    assert_eq!(
        restored.canonical_content().unwrap(),
        value.canonical_content().unwrap()
    );
    assert_eq!(f.role.manifest(value.root_run_id).await, frozen);
}

#[tokio::test]
async fn workflow_role_sponsor_later_people_sql_refuses_informed_capacity_on_other_roles() {
    // Keep a genuine frozen Reviewer requirement so the informed-capacity rule is the
    // distinguishing rejection, rather than a missing requirement for the changed role.
    let f = Box::pin(PeopleFixture::new(2)).await;
    let run = f.role.admitted("reviewer-cannot-be-informed").await;
    let valid = document(&f, run).await;
    let mut invalid = valid.clone();
    invalid.informed = invalid.principals.split_off(1);
    let mut tx = f.role.p().pool().begin().await.unwrap();
    let error = insert_raw_fact(&mut tx, &invalid, &serde_json::to_value(&invalid).unwrap())
        .await
        .unwrap_err();
    rejected(&error, "workflow_guard:guard_workflow_people_resolution");
    tx.rollback().await.unwrap();
    assert_no_later_facts(&f, &valid).await;
    let mut tx = f.role.p().pool().begin().await.unwrap();
    people_resolutions::persist_on(&mut tx, &valid)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn workflow_role_sponsor_later_people_sql_rejects_missing_extra_and_wrong_capacity_members() {
    let (f, value) = Box::pin(sponsor_document()).await;
    let owner = f.role.admission.binding.target.actor.user_id();
    let extra = PrincipalId::new(f.role.admission.binding.fixture.principal(owner).await);
    let content = serde_json::to_value(&value).unwrap();
    for variant in ["missing", "extra", "capacity"] {
        let mut tx = f.role.p().pool().begin().await.unwrap();
        insert_raw_fact(&mut tx, &value, &content).await.unwrap();
        if variant == "capacity" {
            let error = insert_member(&mut tx, &value, value.principals[0], "informed")
                .await
                .unwrap_err();
            rejected(&error, "workflow_guard:guard_workflow_role_binding");
            tx.rollback().await.unwrap();
        } else {
            insert_member(&mut tx, &value, value.principals[0], "decides")
                .await
                .unwrap();
            if variant == "extra" {
                insert_member(&mut tx, &value, value.informed[0], "informed")
                    .await
                    .unwrap();
                let error = insert_member(&mut tx, &value, extra, "informed")
                    .await
                    .unwrap_err();
                rejected(&error, "workflow_guard:guard_workflow_role_binding");
                tx.rollback().await.unwrap();
            } else {
                let error = tx.commit().await.unwrap_err();
                rejected(
                    &error,
                    "workflow_guard:check_workflow_people_members_complete",
                );
            }
        }
        assert_no_later_facts(&f, &value).await;
    }
    let mut tx = f.role.p().pool().begin().await.unwrap();
    insert_raw_fact(&mut tx, &value, &content).await.unwrap();
    insert_member(&mut tx, &value, value.principals[0], "decides")
        .await
        .unwrap();
    insert_member(&mut tx, &value, value.informed[0], "informed")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let mut connection = f.role.p().pool().acquire().await.unwrap();
    let restored =
        people_resolutions::read_on(&mut connection, &key(&value), value.resolution_revision)
            .await
            .unwrap();
    assert_eq!(
        restored.canonical_content().unwrap(),
        value.canonical_content().unwrap()
    );
}
