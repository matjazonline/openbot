//! `department_competing_`: writers the company write lock and the principal lock serialize, lined
//! up behind a held lock on their own database and released together.
use super::*;

/// Hold the company write lock so competitors line up behind it, then release them together.
async fn hold_company(f: &Departments) -> sqlx::Transaction<'_, sqlx::Postgres> {
    let mut blocker = f.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id = $1 FOR NO KEY UPDATE")
        .bind(f.company)
        .execute(&mut *blocker)
        .await
        .unwrap();
    blocker
}

#[tokio::test]
async fn department_competing_reparents_never_form_a_cycle() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = Arc::new(Departments::on(database.pool.clone()).await);
    let east = f.create("East", None).await;
    let west = f.create("West", None).await;
    let blocker = hold_company(&f).await;
    let first = tokio::spawn({
        let f = f.clone();
        async move { f.reparent(east.id, Some(west.id)).await }
    });
    let second = tokio::spawn({
        let f = f.clone();
        async move { f.reparent(west.id, Some(east.id)).await }
    });
    wait_until_backends_are_blocked(f.pool(), 2).await;
    blocker.commit().await.unwrap();
    let outcomes = [first.await.unwrap(), second.await.unwrap()];
    assert_eq!(
        outcomes.iter().filter(|outcome| outcome.is_ok()).count(),
        1,
        "{outcomes:?}"
    );
    assert!(
        outcomes
            .iter()
            .any(|outcome| matches!(outcome, Err(AppError::BadRequest(_)))),
        "{outcomes:?}"
    );
    let parents: Vec<Option<Uuid>> =
        sqlx::query_scalar("SELECT parent_id FROM departments WHERE company_id = $1 ORDER BY slug")
            .bind(f.company)
            .fetch_all(f.pool())
            .await
            .unwrap();
    assert_eq!(parents.iter().filter(|parent| parent.is_some()).count(), 1);
}

#[tokio::test]
async fn department_competing_creates_of_one_slug_leave_one_row() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = Arc::new(Departments::on(database.pool.clone()).await);
    let blocker = hold_company(&f).await;
    let competitors: Vec<_> = (0..2)
        .map(|_| {
            let f = f.clone();
            tokio::spawn(async move {
                f.use_cases
                    .create_department(f.owner, f.company, f.write("Shared", None), None)
                    .await
            })
        })
        .collect();
    wait_until_backends_are_blocked(f.pool(), 2).await;
    blocker.commit().await.unwrap();
    let mut outcomes = Vec::new();
    for competitor in competitors {
        outcomes.push(competitor.await.unwrap());
    }
    assert_eq!(
        outcomes.iter().filter(|o| o.is_ok()).count(),
        1,
        "{outcomes:?}"
    );
    assert!(
        outcomes
            .iter()
            .any(|o| matches!(o, Err(AppError::Conflict(_)))),
        "{outcomes:?}"
    );
    let rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM departments WHERE company_id = $1 AND slug = 'shared'",
    )
    .bind(f.company)
    .fetch_one(f.pool())
    .await
    .unwrap();
    assert_eq!(rows, 1);
}

#[tokio::test]
async fn department_competing_lead_assignments_leave_one_holder() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = Arc::new(Departments::on(database.pool.clone()).await);
    let department = f.create("Ops", None).await;
    let blocker = hold_company(&f).await;
    let competitors: Vec<_> = [f.member_principal, f.agent_principal]
        .into_iter()
        .map(|principal| {
            let f = f.clone();
            tokio::spawn(async move {
                f.use_cases
                    .assign_role(
                        f.owner,
                        f.company,
                        department.id,
                        DepartmentRole::Lead,
                        principal,
                    )
                    .await
            })
        })
        .collect();
    wait_until_backends_are_blocked(f.pool(), 2).await;
    blocker.commit().await.unwrap();
    for competitor in competitors {
        competitor
            .await
            .unwrap()
            .expect("each assignment succeeds in turn");
    }
    let holders = f.roles(department.id).await;
    assert_eq!(holders.len(), 1, "{holders:?}");
    assert!([f.member_principal, f.agent_principal].contains(&holders[0].principal_id));
}

/// An add waiting behind a demotion in flight reads its outcome and refuses, rather than leaving
/// a department row on a principal that has just left the company.
#[tokio::test]
async fn department_competing_member_add_behind_a_demotion_is_refused() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = Arc::new(Departments::on(database.pool.clone()).await);
    let department = f.create("Late", None).await;
    let mut demotion = f.pool().begin().await.unwrap();
    sqlx::query("UPDATE principals SET kind = 'external', user_id = NULL WHERE id = $1")
        .bind(f.member_principal.as_uuid())
        .execute(&mut *demotion)
        .await
        .unwrap();
    let add = tokio::spawn({
        let f = f.clone();
        async move {
            f.use_cases
                .add_member(f.owner, f.company, department.id, f.member_principal)
                .await
        }
    });
    wait_until_backends_are_blocked(f.pool(), 1).await;
    demotion.commit().await.unwrap();
    let outcome = add.await.unwrap();
    assert!(
        matches!(outcome, Err(AppError::BadRequest(_))),
        "{outcome:?}"
    );
    assert!(f.members(department.id).await.is_empty());
}

/// The reverse order of the kind race (01DEP-P01-G1-F10): a demotion that waits behind an add in
/// flight runs its trigger after the add commits and takes the committed row out again.
#[tokio::test]
async fn department_competing_demotion_behind_a_member_add_removes_the_row() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = Arc::new(Departments::on(database.pool.clone()).await);
    let department = f.create("Early", None).await;
    // A real trigger pauses the actual add after its principal lock and its insert.
    sqlx::query(&format!(
        r#"CREATE FUNCTION pause_department_member() RETURNS trigger LANGUAGE plpgsql AS $$
           BEGIN IF NEW.principal_id = '{}'::uuid THEN PERFORM pg_advisory_xact_lock(61017); END IF;
           RETURN NEW; END $$"#,
        f.member_principal
    ))
    .execute(f.pool())
    .await
    .unwrap();
    sqlx::query(
        "CREATE TRIGGER zz_pause_department_member AFTER INSERT ON department_members \
         FOR EACH ROW EXECUTE FUNCTION pause_department_member()",
    )
    .execute(f.pool())
    .await
    .unwrap();
    let mut gate = f.pool().begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(61017)")
        .execute(&mut *gate)
        .await
        .unwrap();
    let add = tokio::spawn({
        let f = f.clone();
        async move {
            f.use_cases
                .add_member(f.owner, f.company, department.id, f.member_principal)
                .await
        }
    });
    wait_until_backends_are_blocked(f.pool(), 1).await;
    let demotion = tokio::spawn({
        let pool = f.pool().clone();
        let principal = f.member_principal;
        async move {
            sqlx::query("UPDATE principals SET kind = 'external', user_id = NULL WHERE id = $1")
                .bind(principal.as_uuid())
                .execute(&pool)
                .await
        }
    });
    wait_until_backends_are_blocked(f.pool(), 2).await;
    gate.commit().await.unwrap();
    add.await.unwrap().expect("the add went first and commits");
    demotion.await.unwrap().expect("the demotion then commits");
    assert!(
        f.members(department.id).await.is_empty(),
        "the demotion's trigger removed the row the add committed"
    );
}

/// An admin demoted while their write waits on the company lock is refused when it gets the lock:
/// persistence re-reads their authority under it rather than trusting the use case's earlier check.
#[tokio::test]
async fn department_competing_write_behind_an_admin_demotion_is_refused() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = Arc::new(Departments::on(database.pool.clone()).await);
    let mut demotion = hold_company(&f).await;
    sqlx::query(
        "UPDATE company_members SET role = 'member' WHERE company_id = $1 AND user_id = $2",
    )
    .bind(f.company)
    .bind(f.admin)
    .execute(&mut *demotion)
    .await
    .unwrap();
    let create = tokio::spawn({
        let f = f.clone();
        async move {
            f.use_cases
                .create_department(f.admin, f.company, f.write("Late Admin", None), None)
                .await
        }
    });
    wait_until_backends_are_blocked(f.pool(), 1).await;
    demotion.commit().await.unwrap();
    let outcome = create.await.unwrap();
    assert!(matches!(outcome, Err(AppError::NotFound(_))), "{outcome:?}");
    let created: i64 = sqlx::query_scalar("SELECT count(*) FROM departments WHERE company_id = $1")
        .bind(f.company)
        .fetch_one(f.pool())
        .await
        .unwrap();
    assert_eq!(created, 0);
}
