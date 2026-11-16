use super::super::reviewer_picker::{ReviewerForm, apply, picker};
use super::*;
use crate::application::ports::channel::{ChannelOwnershipWrite, ChannelPersistence, ChannelWrite};
use crate::application::ports::department::DepartmentWrite;
use crate::application::workflow::registry;
use crate::domain::workflow::CompanyId;
use crate::entities::department::DepartmentRole;

fn form(source: &str, reviewer: &str, desk: Option<Uuid>) -> axum::Form<ReviewerForm> {
    axum::Form(serde_json::from_value(json!({"source":source,"step":"start","reviewer":reviewer,"desk":desk.map(|id|id.to_string())})).unwrap())
}

const HOSTILE_LABEL: &str = "Reviewer </option><img src=x onerror=alert(1)>";

async fn preview_desk(f: &Fixture) -> Uuid {
    let ui = f.ui(f.owner);
    let principal: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND user_id=$2")
            .bind(f.company)
            .bind(f.owner)
            .fetch_one(f.persistence.pool())
            .await
            .unwrap();
    let hostile = HOSTILE_LABEL;
    sqlx::query("UPDATE users SET username=$2 WHERE id=$1")
        .bind(f.owner)
        .bind(hostile)
        .execute(f.persistence.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE principals SET display_label=$3 WHERE company_id=$1 AND id=$2")
        .bind(f.company)
        .bind(principal)
        .bind(hostile)
        .execute(f.persistence.pool())
        .await
        .unwrap();
    let department = ui
        .departments
        .create_department(
            f.owner,
            f.company,
            DepartmentWrite {
                name: "Team".into(),
                slug: "team".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
    ui.departments
        .assign_role(
            f.owner,
            f.company,
            department.id,
            DepartmentRole::Reviewer,
            crate::entities::transport::PrincipalId::new(principal),
        )
        .await
        .unwrap();
    let desk = ChannelPersistence::create(
        &f.persistence,
        f.company,
        ChannelWrite {
            name: "Preview".into(),
            slug: "preview".into(),
            enabled: true,
            ownership: Some(ChannelOwnershipWrite {
                department_id: Some(Some(department.id)),
                members_only: Some(false),
            }),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    desk.id
}

#[tokio::test]
async fn workflow_role_reviewer_picker_previews_scoped_holders_escapes_and_preserves_unsaved_source()
 {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let desk = preview_desk(&f).await;
    let workflow = Uuid::new_v4();
    let mut source: Value =
        serde_json::from_str(&registry::example("decision.human").unwrap().source).unwrap();
    source["workflow_id"] = json!(workflow);
    let source = format!("{}\n# unsaved reviewer edit\n", source);
    f.draft(workflow, "Reviewers", &source).await;
    let html = picker(
        f.ui(f.owner),
        Path(workflow),
        f.query(),
        form(&source, "role:reviewer", Some(desk)),
    )
    .await
    .unwrap()
    .0;
    assert!(html.contains("Today's holders") && html.contains("optgroup label=\"Role\""));
    assert!(
        html.contains("reviewer: Reviewer &lt;/option&gt;&lt;img") && !html.contains("<img src=x")
    );
    assert!(
        html.contains("data-workflow-reviewer-picker")
            && html.contains("Updating reviewer selection")
    );
    let changed = apply(
        f.ui(f.owner),
        Path(workflow),
        f.query(),
        form(&source, "role:reviewer", Some(desk)),
    )
    .await
    .unwrap()
    .0;
    assert!(changed.contains("# unsaved reviewer edit"));
    assert!(changed.contains("&quot;reviewer&quot;"));
    assert_eq!(
        f.ui(f.owner)
            .detail(f.company, workflow)
            .await
            .unwrap()
            .source,
        source
    );
}

#[tokio::test]
async fn workflow_role_reviewer_picker_refuses_foreign_people_desks_and_actors() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    let mut source: Value =
        serde_json::from_str(&registry::example("decision.human").unwrap().source).unwrap();
    source["workflow_id"] = json!(workflow);
    let source = source.to_string();
    f.draft(workflow, "Reviewers", &source).await;
    let foreign = f
        .engine
        .authoring_reads()
        .reviewer_choices(CompanyId::new(f.other), None)
        .await
        .unwrap()
        .items[0]
        .principal;
    assert!(matches!(
        apply(
            f.ui(f.owner),
            Path(workflow),
            f.query(),
            form(&source, &foreign.as_uuid().to_string(), None)
        )
        .await,
        Err(AppError::NotFound(_))
    ));
    assert!(matches!(
        picker(
            f.ui(f.member),
            Path(workflow),
            f.query(),
            form(&source, "role:reviewer", None)
        )
        .await,
        Err(AppError::NotFound(_))
    ));
    let foreign_desk = ChannelPersistence::create(
        &f.persistence,
        f.other,
        ChannelWrite {
            name: "Other".into(),
            slug: "other".into(),
            enabled: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        picker(
            f.ui(f.owner),
            Path(workflow),
            f.query(),
            form(&source, "role:reviewer", Some(foreign_desk.id))
        )
        .await,
        Err(AppError::NotFound(_))
    ));
}
