use super::super::authorization_test_support::{Fixture, denied};
use super::*;
use axum::http::StatusCode;

fn dashboard(f: &Fixture, account: &crate::entities::user::User, caller: Caller) -> Dashboard {
    Dashboard {
        company_use_cases: f.companies.clone(),
        department_use_cases: f.departments(),
        dashboard_snapshots: Arc::new(DashboardSnapshotService::new(f.persistence.clone())),
        database_query_health: Arc::new(DatabaseQueryHealthService::new(f.persistence.clone())),
        dashboard_sse_connections: Arc::new(AtomicU64::new(0)),
        runtime_metrics: f.persistence.clone(),
        runtime_identity: MachineIdentity::process().clone(),
        monitoring: Arc::new(crate::adapters::monitoring::InMemoryMonitor::new()),
        config: f.config.clone(),
        user_id: account.id,
        account: account.clone(),
        caller,
    }
}

#[tokio::test]
async fn dashboard_resolves_operator_company_before_managed_selection_and_refuses_member_or_stranger()
 {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let query = |company_id| DashboardQuery {
        company_id: Some(company_id),
        scope: None,
        window: None,
        department_id: None,
    };
    let operator = dashboard(&f, &f.owner, Caller::operator(f.owner.id));
    let scope = DashboardScope::resolve(&operator, &query(f.foreign.id))
        .await
        .unwrap();
    assert_eq!(scope.company_id(), Some(f.foreign.id));
    assert_eq!(scope.membership(), CompanyMembership::None);
    assert_eq!(scope.selected_company.unwrap().name, f.foreign.name);
    let member = dashboard(&f, &f.member, Caller::user(f.member.id));
    denied(
        DashboardScope::resolve(&member, &query(f.company.id)).await,
        StatusCode::FORBIDDEN,
        &f.company.name,
    )
    .await;
    let stranger = dashboard(&f, &f.stranger, Caller::user(f.stranger.id));
    denied(
        DashboardScope::resolve(&stranger, &query(f.company.id)).await,
        StatusCode::NOT_FOUND,
        &f.company.name,
    )
    .await;
}

/// 01-departments 04C.1: a company view reads its departments; a department id that is not the
/// company's is not found, not ignored.
#[tokio::test]
async fn department_views_dashboard_department_must_be_the_companys() {
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
    let owner = dashboard(&f, &f.owner, Caller::user(f.owner.id));
    let query = |department_id| DashboardQuery {
        company_id: Some(f.company.id),
        scope: None,
        window: None,
        department_id,
    };
    let scope = DashboardScope::resolve(&owner, &query(Some(sales.id.as_uuid())))
        .await
        .unwrap();
    assert_eq!(scope.department, Some(sales.id));
    assert!(scope.departments.department(sales.id).is_some());
    assert!(matches!(
        DashboardScope::resolve(&owner, &query(Some(Uuid::new_v4()))).await,
        Err(AppError::NotFound(_))
    ));
}
