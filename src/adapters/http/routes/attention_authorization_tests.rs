use super::super::authorization_test_support::{Fixture, denied};
use super::*;
use axum::http::StatusCode;

#[tokio::test]
async fn attention_application_gating_refuses_team_work_and_hides_foreign_tenants() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let query = |view: &str| {
        Query(ListQuery {
            view: Some(view.into()),
            cursor: None,
            limit: None,
            department_id: None,
        })
    };
    denied(
        list_attention(
            State(f.attention.clone()),
            State(f.departments()),
            Fixture::viewer(&f.owner),
            Path(f.foreign.id),
            query("my_work"),
        )
        .await,
        StatusCode::NOT_FOUND,
        &f.foreign.name,
    )
    .await;
    denied(
        list_attention(
            State(f.attention.clone()),
            State(f.departments()),
            Fixture::viewer(&f.member),
            Path(f.company.id),
            query("team_work"),
        )
        .await,
        StatusCode::FORBIDDEN,
        &f.company.name,
    )
    .await;
    denied(
        operational_summary(
            State(f.attention.clone()),
            Fixture::viewer(&f.member),
            Path(f.company.id),
        )
        .await,
        StatusCode::FORBIDDEN,
        &f.company.name,
    )
    .await;
    assert!(
        list_attention(
            State(f.attention.clone()),
            State(f.departments()),
            Fixture::viewer(&f.member),
            Path(f.company.id),
            query("my_work")
        )
        .await
        .is_ok()
    );
    let failing = Arc::new(AttentionUseCases::new(
        f.persistence.clone(),
        Arc::new(crate::use_cases::company_test_support::MockCompanyPersistence::failing()),
        f.persistence.clone(),
        f.persistence.clone(),
        Arc::new(crate::adapters::monitoring::InMemoryMonitor::new()),
    ));
    denied(
        list_attention(
            State(failing),
            State(f.departments()),
            Fixture::viewer(&f.owner),
            Path(f.company.id),
            query("my_work"),
        )
        .await,
        StatusCode::INTERNAL_SERVER_ERROR,
        &f.company.name,
    )
    .await;
}

/// 01-departments 04C.1: a department filter names one of the company's departments, or the
/// JSON queue answers not found; the company's own narrows the list.
#[tokio::test]
async fn department_views_attention_department_must_be_the_companys() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let sales = f
        .departments()
        .create_department(
            f.owner.id,
            f.company.id,
            crate::application::ports::department::DepartmentWrite {
                name: "Sales".into(),
                slug: "sales".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
    let query = |department_id| {
        Query(ListQuery {
            view: Some("team_work".into()),
            cursor: None,
            limit: None,
            department_id,
        })
    };
    let listed = list_attention(
        State(f.attention.clone()),
        State(f.departments()),
        Fixture::viewer(&f.owner),
        Path(f.company.id),
        query(Some(sales.id.as_uuid())),
    )
    .await
    .unwrap();
    assert!(listed.0.items.is_empty());
    denied(
        list_attention(
            State(f.attention.clone()),
            State(f.departments()),
            Fixture::viewer(&f.owner),
            Path(f.company.id),
            query(Some(Uuid::new_v4())),
        )
        .await,
        StatusCode::NOT_FOUND,
        &f.company.name,
    )
    .await;
}

/// Records the query the use case reads with, answering an empty page.
struct RecordedQuery(std::sync::Mutex<Vec<(Vec<Uuid>, bool, bool)>>);

#[async_trait::async_trait]
impl crate::application::attention::AttentionPersistence for RecordedQuery {
    async fn list_attention(
        &self,
        query: crate::entities::attention::AttentionQuery<'_>,
    ) -> crate::app_error::AppResult<AttentionPage> {
        self.0.lock().unwrap().push((
            query.visible_channel_ids.to_vec(),
            query.manager,
            query.company_items,
        ));
        Ok(AttentionPage {
            items: Vec::new(),
            next_cursor: None,
            as_of: chrono::Utc::now(),
            working_set_size: 0,
            truncated: false,
        })
    }

    async fn operational_summary(
        &self,
        _company_id: Uuid,
        _visible_channel_ids: &[Uuid],
    ) -> crate::app_error::AppResult<crate::entities::attention::OperationalSummary> {
        unimplemented!()
    }
}

/// 01-departments 04C.1 (B2-F4): a one-department view reads only the visible desks that
/// department owns, and no company-associated items; a manager keeps the run links.
#[tokio::test]
async fn department_views_attention_reads_only_the_departments_desks() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let owned = crate::application::ports::channel::ChannelPersistence::create(
        f.persistence.as_ref(),
        f.company.id,
        crate::application::ports::channel::ChannelWrite {
            name: "Sales desk".into(),
            slug: "sales-desk".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let sales = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO departments (id, company_id, name, slug, created_by) VALUES ($1, $2, 'Sales', \
         'sales', '{\"actor_type\":\"system\",\"actor_id\":null,\"actor_name\":\"Test\"}')",
    )
    .bind(sales)
    .bind(f.company.id)
    .execute(f.persistence.pool())
    .await
    .unwrap();
    sqlx::query("UPDATE channels SET department_id = $1 WHERE id = $2")
        .bind(sales)
        .bind(owned)
        .execute(f.persistence.pool())
        .await
        .unwrap();
    let recorded = Arc::new(RecordedQuery(Default::default()));
    let attention = crate::application::attention::AttentionUseCases::new(
        recorded.clone(),
        f.persistence.clone(),
        f.persistence.clone(),
        f.persistence.clone(),
        Arc::new(crate::adapters::monitoring::InMemoryMonitor::new()),
    );
    let request = |department| crate::application::attention::AttentionListRequest {
        view: AttentionView::TeamWork,
        cursor: None,
        limit: None,
        department,
    };
    let everything = attention
        .list(&Fixture::viewer(&f.owner), f.company.id, request(None))
        .await
        .unwrap();
    assert!(everything.readable_desks.contains(&owned));
    assert!(
        everything.readable_desks.len() > 1,
        "another desk is visible too"
    );
    attention
        .list(
            &Fixture::viewer(&f.owner),
            f.company.id,
            request(Some(crate::entities::department::DepartmentId::new(sales))),
        )
        .await
        .unwrap();
    let queries = recorded.0.lock().unwrap().clone();
    assert_eq!(queries.len(), 2);
    let (all, manager, company_items) = &queries[0];
    assert_eq!(all.len(), everything.readable_desks.len());
    assert!(*manager && *company_items);
    assert_eq!(queries[1], (vec![owned], true, false));
}

/// 01-departments 04C.1 (B2-F1): a plain member's departments are the ones they belong to or lead
/// and the owners of the desks they can read; another department is not found as a filter.
#[tokio::test]
async fn department_views_a_member_filters_only_by_departments_they_may_see() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let departments = f.departments();
    let write = |slug: &str| crate::application::ports::department::DepartmentWrite {
        name: slug.into(),
        slug: slug.into(),
        ..Default::default()
    };
    let mine = departments
        .create_department(f.owner.id, f.company.id, write("mine"), None)
        .await
        .unwrap();
    let hidden = departments
        .create_department(f.owner.id, f.company.id, write("hidden"), None)
        .await
        .unwrap();
    let member: Uuid = sqlx::query_scalar(
        "SELECT id FROM principals WHERE company_id = $1 AND user_id = $2 AND kind = 'person'",
    )
    .bind(f.company.id)
    .bind(f.member.id)
    .fetch_one(f.persistence.pool())
    .await
    .unwrap();
    departments
        .add_member(
            f.owner.id,
            f.company.id,
            mine.id,
            crate::entities::transport::PrincipalId::new(member),
        )
        .await
        .unwrap();
    // The member reads none of `hidden`'s desks (its own desk is a team desk they could read,
    // so take it out of what they read here).
    let seen = departments
        .desk_departments(f.member.id, f.company.id, &[])
        .await
        .unwrap();
    assert!(seen.department(mine.id).is_some());
    assert!(seen.department(hidden.id).is_none());
    // A desk they can read names its department.
    let hidden_desk: Uuid = sqlx::query_scalar("SELECT id FROM channels WHERE department_id = $1")
        .bind(hidden.id.as_uuid())
        .fetch_one(f.persistence.pool())
        .await
        .unwrap();
    let through_desk = departments
        .desk_departments(f.member.id, f.company.id, &[hidden_desk])
        .await
        .unwrap();
    assert!(through_desk.department(hidden.id).is_some());
    assert_eq!(
        through_desk.of_desk(Some(hidden_desk)).map(|d| d.id),
        Some(hidden.id)
    );
    // An owner sees every department.
    let all = departments
        .desk_departments(f.owner.id, f.company.id, &[])
        .await
        .unwrap();
    assert!(all.department(hidden.id).is_some() && all.department(mine.id).is_some());

    let query = |department: Uuid| {
        Query(ListQuery {
            view: Some("my_work".into()),
            cursor: None,
            limit: None,
            department_id: Some(department),
        })
    };
    let listed = list_attention(
        State(f.attention.clone()),
        State(f.departments()),
        Fixture::viewer(&f.member),
        Path(f.company.id),
        query(mine.id.as_uuid()),
    )
    .await
    .unwrap();
    assert!(listed.0.items.is_empty());
    // `hidden`'s own desk is a team desk the member can read, so it is named — make it
    // members-only of `hidden` first, which takes it out of what the member reads.
    sqlx::query("UPDATE channels SET access_mode = 'allowlist', members_only = true WHERE id = $1")
        .bind(hidden_desk)
        .execute(f.persistence.pool())
        .await
        .unwrap();
    denied(
        list_attention(
            State(f.attention.clone()),
            State(f.departments()),
            Fixture::viewer(&f.member),
            Path(f.company.id),
            query(hidden.id.as_uuid()),
        )
        .await,
        StatusCode::NOT_FOUND,
        &f.company.name,
    )
    .await;
}
