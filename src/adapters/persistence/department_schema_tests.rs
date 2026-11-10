//! `department_schema_`: what the schema refuses or does without the application.
use super::*;

#[tokio::test]
async fn department_schema_refuses_a_cycle_and_its_own_parent() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let root = f.insert_raw(f.company, "root", None).await.unwrap();
    let child = f.insert_raw(f.company, "child", Some(root)).await.unwrap();
    let grandchild = f
        .insert_raw(f.company, "grandchild", Some(child))
        .await
        .unwrap();
    let cycle = sqlx::query("UPDATE departments SET parent_id = $2 WHERE id = $1")
        .bind(root)
        .bind(grandchild)
        .execute(f.pool())
        .await;
    assert_eq!(refused_by(cycle), "departments_shape");
    let own = sqlx::query("UPDATE departments SET parent_id = id WHERE id = $1")
        .bind(child)
        .execute(f.pool())
        .await;
    assert_eq!(refused_by(own), "departments_shape");
}

#[tokio::test]
async fn department_schema_refuses_a_ninth_level_and_a_subtree_moved_too_deep() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let mut chain = Vec::new();
    for level in 1..=MAX_DEPARTMENT_DEPTH {
        let parent = chain.last().copied();
        chain.push(
            f.insert_raw(f.company, &format!("level-{level}"), parent)
                .await
                .expect("eight levels nest"),
        );
    }
    let ninth = f
        .insert_raw(f.company, "level-9", chain.last().copied())
        .await;
    assert_eq!(refused_by(ninth), "departments_shape");

    // Moving level 2 (a subtree seven deep) under a second-level node would reach level 9.
    let other = f.insert_raw(f.company, "other", None).await.unwrap();
    let other_child = f
        .insert_raw(f.company, "other-child", Some(other))
        .await
        .unwrap();
    let too_deep = sqlx::query("UPDATE departments SET parent_id = $2 WHERE id = $1")
        .bind(chain[1])
        .bind(other_child)
        .execute(f.pool())
        .await;
    assert_eq!(refused_by(too_deep), "departments_shape");
    // Under a root it stays at eight.
    sqlx::query("UPDATE departments SET parent_id = $2 WHERE id = $1")
        .bind(chain[1])
        .bind(other)
        .execute(f.pool())
        .await
        .expect("a subtree seven deep fits under a root");
}

#[tokio::test]
async fn department_schema_refuses_cross_company_parents_and_principals() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let foreign = f
        .insert_raw(f.foreign_company, "foreign", None)
        .await
        .unwrap();
    let cross_parent = f.insert_raw(f.company, "stray", Some(foreign)).await;
    assert_eq!(refused_by(cross_parent), "departments_parent_fk");

    let department = f.insert_raw(f.company, "home", None).await.unwrap();
    let cross_lead = raw_role(f.company, department, "lead", f.foreign_principal)
        .execute(f.pool())
        .await;
    assert_eq!(refused_by(cross_lead), "department_roles_principal_fk");
    let cross_member = raw_member(f.company, department, f.foreign_principal)
        .execute(f.pool())
        .await;
    assert_eq!(refused_by(cross_member), "department_members_principal_fk");
    // A row claiming the other company for this company's department is refused too.
    let cross_department = raw_member(f.foreign_company, department, f.foreign_principal)
        .execute(f.pool())
        .await;
    assert_eq!(
        refused_by(cross_department),
        "department_members_department_fk"
    );
}

#[tokio::test]
async fn department_schema_refuses_external_and_system_principals() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let department = f.insert_raw(f.company, "ops", None).await.unwrap();
    for principal in [f.external_principal, f.system_principal] {
        let lead = raw_role(f.company, department, "lead", principal)
            .execute(f.pool())
            .await;
        assert_eq!(refused_by(lead), "department_principal_kind");
        let member = raw_member(f.company, department, principal)
            .execute(f.pool())
            .await;
        assert_eq!(refused_by(member), "department_principal_kind");
    }
    raw_member(f.company, department, f.agent_principal)
        .execute(f.pool())
        .await
        .expect("an agent may be a member");
}

#[tokio::test]
async fn department_schema_refuses_a_second_lead_and_an_unknown_role() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let department = f.insert_raw(f.company, "sales", None).await.unwrap();
    raw_role(f.company, department, "lead", f.owner_principal)
        .execute(f.pool())
        .await
        .expect("one lead");
    let second = raw_role(f.company, department, "lead", f.member_principal)
        .execute(f.pool())
        .await;
    assert_eq!(refused_by(second), "department_roles_one_lead");
    let unknown = raw_role(f.company, department, "manager", f.member_principal)
        .execute(f.pool())
        .await;
    assert_eq!(refused_by(unknown), "department_roles_role_check");
}

#[tokio::test]
async fn department_schema_cascades_members_and_roles_and_keeps_children() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let parent = f.insert_raw(f.company, "parent", None).await.unwrap();
    let child = f.insert_raw(f.company, "kid", Some(parent)).await.unwrap();
    raw_member(f.company, child, f.member_principal)
        .execute(f.pool())
        .await
        .unwrap();
    raw_role(f.company, child, "lead", f.owner_principal)
        .execute(f.pool())
        .await
        .unwrap();

    let orphaning = sqlx::query("DELETE FROM departments WHERE id = $1")
        .bind(parent)
        .execute(f.pool())
        .await;
    assert_eq!(refused_by(orphaning), "departments_parent_fk");

    sqlx::query("DELETE FROM departments WHERE id = $1")
        .bind(child)
        .execute(f.pool())
        .await
        .expect("a leaf department deletes");
    let left: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM department_members WHERE department_id = $1), \
                (SELECT count(*) FROM department_roles WHERE department_id = $1)",
    )
    .bind(child)
    .fetch_one(f.pool())
    .await
    .unwrap();
    assert_eq!(left, (0, 0), "members and roles go with the department");
}

#[tokio::test]
async fn department_schema_tears_down_a_nested_tree_with_its_company() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let root = f.insert_raw(f.company, "hq", None).await.unwrap();
    let child = f.insert_raw(f.company, "branch", Some(root)).await.unwrap();
    raw_member(f.company, child, f.agent_principal)
        .execute(f.pool())
        .await
        .unwrap();
    let mut tx = f.pool().begin().await.unwrap();
    sqlx::query("DELETE FROM companies WHERE id = $1")
        .bind(f.company)
        .execute(&mut *tx)
        .await
        .expect("a company teardown removes its whole department tree");
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM departments WHERE company_id = $1")
        .bind(f.company)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(left, 0);
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn department_schema_drops_a_demoted_principal_from_every_department() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let first = f.insert_raw(f.company, "first", None).await.unwrap();
    let second = f.insert_raw(f.company, "second", None).await.unwrap();
    for department in [first, second] {
        raw_member(f.company, department, f.member_principal)
            .execute(f.pool())
            .await
            .unwrap();
    }
    raw_role(f.company, first, "lead", f.member_principal)
        .execute(f.pool())
        .await
        .unwrap();
    raw_member(f.company, second, f.agent_principal)
        .execute(f.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE principals SET kind = 'external', user_id = NULL WHERE id = $1")
        .bind(f.member_principal.as_uuid())
        .execute(f.pool())
        .await
        .unwrap();
    assert!(f.members(DepartmentId::new(first)).await.is_empty());
    assert_eq!(
        f.members(DepartmentId::new(second)).await,
        vec![f.agent_principal],
        "only the demoted principal leaves"
    );
    assert!(f.roles(DepartmentId::new(first)).await.is_empty());
}

/// The Rust enum and `channel_principal_grants_provenance_check` list the same strings.
#[tokio::test]
async fn department_schema_grant_provenance_check_accepts_every_variant() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let channel = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO channels (id, company_id, name, access_mode, created_by) \
         VALUES ($1, $2, 'Grants', 'allowlist', $3)",
    )
    .bind(channel)
    .bind(f.company)
    .bind(json!({"actor_type": "system", "actor_id": null, "actor_name": "Test"}))
    .execute(f.pool())
    .await
    .unwrap();
    // One row per provenance: the key includes it, so a principal may hold one capability from
    // several provenances at once (DEP-P02-D1).
    let mut tx = f.pool().begin().await.unwrap();
    for provenance in GrantProvenance::ALL {
        sqlx::query(
            "INSERT INTO channel_principal_grants \
                 (company_id, channel_id, principal_id, capability, provenance) \
             VALUES ($1, $2, $3, 'view', $4)",
        )
        .bind(f.company)
        .bind(channel)
        .bind(f.member_principal.as_uuid())
        .bind(provenance.as_str())
        .execute(&mut *tx)
        .await
        .unwrap_or_else(|error| panic!("{provenance:?} is accepted: {error}"));
    }
    let rows: i64 =
        sqlx::query_scalar("SELECT count(*) FROM channel_principal_grants WHERE channel_id = $1")
            .bind(channel)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(rows, GrantProvenance::ALL.len() as i64);
    let unknown = sqlx::query(
        "INSERT INTO channel_principal_grants \
             (company_id, channel_id, principal_id, capability, provenance) \
         VALUES ($1, $2, $3, 'participate', 'department')",
    )
    .bind(f.company)
    .bind(channel)
    .bind(f.member_principal.as_uuid())
    .execute(&mut *tx)
    .await;
    assert_eq!(
        refused_by(unknown),
        "channel_principal_grants_provenance_check"
    );
}
