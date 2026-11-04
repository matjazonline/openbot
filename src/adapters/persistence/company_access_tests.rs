use super::*;
use crate::adapters::persistence::test_support::test_pool;
use crate::use_cases::user::UserPersistence;
use std::sync::Arc;

#[tokio::test]
async fn company_access_reaches_the_oldest_of_201_companies_outside_the_switcher_cap() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let p = PostgresPersistence::new(pool);
    let suffix = Uuid::new_v4().simple().to_string();
    let owner = p
        .create_user(
            &format!("cap-{suffix}"),
            &format!("cap-{suffix}@example.test"),
            "hash",
        )
        .await
        .unwrap();
    let mut oldest = None;
    for index in 0..201 {
        let company = CompanyPersistence::create(
            &p,
            owner.id,
            CompanyWrite {
                name: format!("Company {index}"),
                slug: format!("cap-{suffix}-{index}"),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        if oldest.is_none() {
            sqlx::query("UPDATE companies SET created_at = CURRENT_TIMESTAMP - INTERVAL '1 year' WHERE id = $1")
                .bind(company.id).execute(p.pool()).await.expect("oldest fixture");
            oldest = Some(company.id);
        }
    }
    let oldest = oldest.unwrap();
    let switcher = p.list_accessible_by_user_id(owner.id).await.unwrap();
    assert_eq!(switcher.len(), 200);
    assert!(!switcher.iter().any(|access| access.company.id == oldest));
    let access = p
        .company_access(owner.id, oldest)
        .await
        .unwrap()
        .expect("one-row lookup is uncapped");
    assert_eq!(access.company.id, oldest);
    assert_eq!(access.membership, CompanyMembership::Owner);
    let use_cases = crate::use_cases::company::CompanyUseCases::new(Arc::new(p));
    assert_eq!(
        use_cases.owned_company(owner.id, oldest).await.unwrap().id,
        oldest
    );
}
