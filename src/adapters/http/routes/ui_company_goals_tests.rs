//! `goal_rollup_` (Plan08 03D.2): the company's Goals tab lists the goals its viewer may see,
//! active or archived, with their counts; nobody else's.
use super::super::authorization_test_support::Fixture;
use super::skills_tests::workspace;
use super::*;
use crate::application::ports::department::{DepartmentWrite, GoalWrite, OpenWork};
use crate::entities::department::DepartmentId;
use crate::entities::transport::PrincipalId;

async fn goal(f: &Fixture, name: &str, sponsor: Uuid) -> DepartmentId {
    f.departments()
        .create_department(
            f.owner.id,
            f.company.id,
            DepartmentWrite {
                name: name.into(),
                slug: name.to_lowercase(),
                goal: Some(GoalWrite {
                    target_date: Some(chrono::NaiveDate::from_ymd_opt(2027, 3, 31).unwrap()),
                    sponsors: vec![PrincipalId::new(sponsor)],
                }),
                ..DepartmentWrite::default()
            },
            None,
        )
        .await
        .unwrap()
        .id
}

/// The table's rows: the part of the page from the goals table on.
fn table(html: &str) -> &str {
    let start = html.find("id=\"goals-table\"").unwrap_or(html.len());
    &html[start..]
}

#[tokio::test]
async fn goal_rollup_goals_page_lists_visible_goals() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let owner: Uuid = sqlx::query_scalar(
        "SELECT id FROM principals WHERE company_id=$1 AND user_id=$2 AND kind='person'",
    )
    .bind(f.company.id)
    .bind(f.owner.id)
    .fetch_one(f.persistence.pool())
    .await
    .unwrap();
    let launch = goal(&f, "Launch", owner).await;
    let old = goal(&f, "Wrapped", owner).await;
    f.departments()
        .archive_department(f.owner.id, f.company.id, old, OpenWork::Refuse)
        .await
        .unwrap();
    f.departments()
        .create_department(
            f.owner.id,
            f.company.id,
            DepartmentWrite {
                name: "Standing".into(),
                slug: "standing".into(),
                ..DepartmentWrite::default()
            },
            None,
        )
        .await
        .unwrap();
    let opened = |user, goal: DepartmentId| {
        companies_page(
            workspace(&f, user),
            Query(WorkspaceQuery {
                company_id: Some(f.company.id),
                tab: Some("goals".into()),
                goal_id: Some(goal.as_uuid()),
                ..WorkspaceQuery::default()
            }),
        )
    };
    let page = |user, archived: Option<&str>| {
        companies_page(
            workspace(&f, user),
            Query(WorkspaceQuery {
                company_id: Some(f.company.id),
                tab: Some("goals".into()),
                archived: archived.map(str::to_owned),
                ..WorkspaceQuery::default()
            }),
        )
    };
    let active = page(&f.owner, None).await.unwrap().0;
    assert!(active.contains("id=\"goals-section\""), "{active}");
    let rows = table(&active);
    assert!(rows.contains("Launch"), "{active}");
    assert!(rows.contains("2027-03-31"), "{active}");
    assert!(!rows.contains("Wrapped") && !rows.contains("Standing"));
    assert!(
        rows.contains(&format!(
            "/ui/companies?company_id={}&amp;tab=goals&amp;goal_id={}",
            f.company.id, launch
        )),
        "{active}"
    );
    assert!(active.contains("archived=true"), "the archived toggle");
    let archived = page(&f.owner, Some("true")).await.unwrap().0;
    let rows = table(&archived);
    assert!(
        rows.contains("Wrapped") && !rows.contains("Launch"),
        "{archived}"
    );
    let card = opened(&f.owner, launch).await.unwrap().0;
    assert!(card.contains("id=\"goal-status\""), "{card}");
    // A member sees a goal only through one of its desks, and then opens its status here,
    // though its department page stays closed to them (08G-P03-B2-F2).
    let member = page(&f.member, None).await.unwrap().0;
    assert!(!table(&member).contains("Launch"), "{member}");
    assert!(matches!(
        opened(&f.member, launch).await,
        Err(crate::app_error::AppError::NotFound(_))
    ));
    let desk = crate::adapters::persistence::test_support::create_channel(
        f.persistence.as_ref(),
        f.company.id,
        crate::application::ports::channel::ChannelWrite {
            name: "Launch desk".into(),
            slug: "launch-desk".into(),
            department_id: Some(launch),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    sqlx::query("UPDATE channels SET access_mode='public' WHERE id=$1")
        .bind(desk)
        .execute(f.persistence.pool())
        .await
        .unwrap();
    let member = page(&f.member, None).await.unwrap().0;
    assert!(table(&member).contains("Launch"), "{member}");
    let card = opened(&f.member, launch).await.unwrap().0;
    assert!(card.contains("id=\"goal-status\""), "{card}");
    assert!(
        f.departments()
            .visible_department(f.member.id, f.company.id, launch)
            .await
            .is_err(),
        "the department page stays closed to a desk viewer"
    );
    let stranger = page(&f.stranger, None).await;
    assert!(stranger.is_err(), "a stranger reads no company");
}
