use super::*;
use crate::application::ports::department::DepartmentWrite;

#[tokio::test]
async fn workflow_reviewer_follow_context_is_original_desk_not_chosen_ancestor() {
    let f = RoleFixture::new().await;
    let child = crate::adapters::persistence::test_support::department_use_cases(f.p())
        .create_department(
            f.admission.binding.target.actor.user_id(),
            f.company().as_uuid(),
            DepartmentWrite {
                name: "Child".into(),
                slug: "child".into(),
                parent_id: Some(f.department),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
    let set_context = async |department| {
        let mut tx = f.p().pool().begin().await.unwrap();
        crate::adapters::persistence::workflow::authority::authorize_company(
            &mut tx,
            f.company(),
            f.admission.binding.target.actor,
        )
        .await
        .unwrap();
        crate::adapters::persistence::department_desks::set_desk_ownership_on(
            &mut tx,
            f.company(),
            f.channel,
            &ChannelOwnershipWrite {
                department_id: Some(Some(department)),
                members_only: None,
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
    };
    set_context(child.id).await;
    let run = f.admitted("frozen-desk-context").await;
    let manifest = f.manifest(run).await;
    assert_eq!(manifest.role_context_department_id, Some(child.id));
    assert!(
        manifest
            .roles
            .iter()
            .all(|role| role.department_id == f.department)
    );
    let normalized: Uuid = sqlx::query_scalar("SELECT role_context_department_id FROM agent_dependency_manifests WHERE company_id=$1 AND root_run_id=$2")
        .bind(f.company().as_uuid()).bind(run.as_uuid()).fetch_one(f.p().pool()).await.unwrap();
    assert_eq!(normalized, child.id.as_uuid());
    set_context(f.department).await;
    assert_eq!(f.manifest(run).await, manifest);
    let next = f.admitted("new-desk-context").await;
    assert_eq!(
        f.manifest(next).await.role_context_department_id,
        Some(f.department)
    );
}

#[tokio::test]
async fn workflow_reviewer_follow_context_rejects_forged_missing_and_mutated_snapshot() {
    let f = RoleFixture::new().await;
    let run = f.admitted("sealed-desk-context").await;
    let saved = f.manifest(run).await;
    for mode in ["missing", "forged", "mismatched"] {
        let mut content = serde_json::to_value(&saved).unwrap();
        let normalized = match mode {
            "missing" => {
                content
                    .as_object_mut()
                    .unwrap()
                    .remove("role_context_department_id");
                Some(f.department.as_uuid())
            }
            "forged" => {
                content["role_context_department_id"] = json!(Uuid::new_v4());
                Some(Uuid::new_v4())
            }
            "mismatched" => None,
            _ => unreachable!(),
        };
        // BEFORE INSERT guard rejects this before the existing-root uniqueness check.
        let error = sqlx::query("INSERT INTO agent_dependency_manifests(company_id,id,root_run_id,content,digest,role_context_department_id) VALUES($1,$2,$3,$4,sha256(convert_to($4,'UTF8')),$5)")
            .bind(f.company().as_uuid()).bind(Uuid::new_v4()).bind(run.as_uuid()).bind(content.to_string()).bind(normalized)
            .execute(f.p().pool()).await.unwrap_err();
        assert_eq!(
            error
                .as_database_error()
                .and_then(|error| error.constraint()),
            Some("workflow_guard:guard_workflow_role_context"),
            "{mode}: {error}"
        );
    }
    let error = sqlx::query("UPDATE agent_dependency_manifests SET role_context_department_id=NULL WHERE company_id=$1 AND root_run_id=$2")
        .bind(f.company().as_uuid()).bind(run.as_uuid()).execute(f.p().pool()).await.unwrap_err();
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("23514")
    );
    assert_eq!(f.manifest(run).await, saved);
}
