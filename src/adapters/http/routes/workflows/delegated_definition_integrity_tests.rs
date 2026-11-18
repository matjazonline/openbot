use super::*;

#[tokio::test]
async fn delegated_admin_definition_ownership_is_immutable_tenant_scoped_and_root_defaulted() {
    let Some(m) = ManagedDefinitions::new().await else {
        return;
    };
    let (workflow, _) = m.published().await;
    let p = &m.f.persistence;
    let mut tx = p.pool().begin().await.unwrap();
    let error = sqlx::query(
        "UPDATE workflow_definitions SET managing_department_id=$3 WHERE company_id=$1 AND id=$2",
    )
    .bind(m.f.company)
    .bind(workflow)
    .bind(m.b.as_uuid())
    .execute(&mut *tx)
    .await
    .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    tx.rollback().await.unwrap();
    assert_eq!(m.creating_unit(workflow).await, m.a.as_uuid());
    let foreign: Uuid =
        sqlx::query_scalar("SELECT id FROM departments WHERE company_id=$1 AND is_root")
            .bind(m.f.other)
            .fetch_one(p.pool())
            .await
            .unwrap();
    let mut tx = p.pool().begin().await.unwrap();
    sqlx::query("INSERT INTO workflow_definitions(company_id,id,managing_department_id,revision,title,description,source) VALUES($1,$2,$3,1,'Invalid','',$4)")
        .bind(m.f.company).bind(Uuid::new_v4()).bind(foreign).bind(example("data.map", Uuid::new_v4()).to_string())
        .execute(&mut *tx).await.unwrap();
    let error = tx.commit().await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("workflow_definitions_managing_department_fk")
    );
    let root: Uuid =
        sqlx::query_scalar("SELECT id FROM departments WHERE company_id=$1 AND is_root")
            .bind(m.f.company)
            .fetch_one(p.pool())
            .await
            .unwrap();
    let native = Uuid::new_v4();
    let stored: Uuid = sqlx::query_scalar("INSERT INTO workflow_definitions(company_id,id,revision,title,description,source) VALUES($1,$2,1,'Trusted native','',$3) RETURNING managing_department_id")
        .bind(m.f.company).bind(native).bind(example("data.map", native).to_string()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(stored, root);
}

#[tokio::test]
async fn delegated_admin_definition_multiple_creator_units_default_to_root_and_company_teardown_succeeds()
 {
    let Some(m) = ManagedDefinitions::new().await else {
        return;
    };
    let p = &m.f.persistence;
    sqlx::query("INSERT INTO department_members(company_id,department_id,principal_id) SELECT $1,unit.id,principal.id FROM departments AS unit CROSS JOIN principals AS principal WHERE unit.company_id=$1 AND unit.id=ANY($2) AND principal.company_id=$1 AND principal.user_id=$3")
        .bind(m.f.company).bind(vec![m.a.as_uuid(),m.b.as_uuid()]).bind(m.f.owner).execute(p.pool()).await.unwrap();
    let workflow = Uuid::new_v4();
    let (status, saved) =
        m.f.save(
            m.f.owner,
            m.f.company,
            workflow,
            None,
            &example("data.map", workflow),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let root: Uuid =
        sqlx::query_scalar("SELECT id FROM departments WHERE company_id=$1 AND is_root")
            .bind(m.f.company)
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(m.creating_unit(workflow).await, root);
    let (owned, _) = m.published().await;
    let desks: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM channels WHERE company_id=$1 AND department_id=$2")
            .bind(m.f.company)
            .bind(m.a.as_uuid())
            .fetch_all(p.pool())
            .await
            .unwrap();
    for desk in desks {
        ChannelPersistence::delete(
            p,
            WorkflowActor::authenticated(m.f.owner).unwrap(),
            m.f.company,
            desk,
        )
        .await
        .unwrap();
    }
    let mut tx = p.pool().begin().await.unwrap();
    sqlx::query("DELETE FROM departments WHERE company_id=$1 AND id=$2")
        .bind(m.f.company)
        .bind(m.a.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    let error = tx.commit().await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("workflow_definitions_managing_department_fk")
    );
    let refused = crate::adapters::persistence::test_support::department_use_cases(p)
        .delete_department(m.f.owner, m.f.company, m.a)
        .await;
    assert!(matches!(
        refused,
        Err(crate::app_error::AppError::Conflict(_))
    ));
    assert_eq!(m.creating_unit(owned).await, m.a.as_uuid());
    CompanyPersistence::delete_for_user(p, m.f.owner, m.f.company)
        .await
        .unwrap();
    assert!(
        CompanyPersistence::get_by_id(p, m.f.company)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        CompanyPersistence::get_by_id(p, m.f.other)
            .await
            .unwrap()
            .is_some()
    );
}
