use super::*;
#[path = "company_invite_level_test_support.rs"]
mod support;
use support::{Endpoint, Request, RouteFixture, only_level_changed};

#[tokio::test]
async fn access_level_legacy_html_member_route_enforces_actor_scope_and_caps() {
    check_member_endpoint(Endpoint::MemberHtml).await;
}
#[tokio::test]
async fn access_level_json_member_route_enforces_actor_scope_and_caps() {
    check_member_endpoint(Endpoint::MemberJson).await;
}
#[tokio::test]
async fn access_level_legacy_html_invite_route_enforces_actor_scope_and_caps() {
    check_invite_endpoint(Endpoint::InviteHtml).await;
}
#[tokio::test]
async fn access_level_json_invite_route_enforces_actor_scope_and_caps() {
    check_invite_endpoint(Endpoint::InviteJson).await;
}

async fn check_member_endpoint(endpoint: Endpoint) {
    let Some(f) = RouteFixture::new().await else {
        return;
    };
    let base = &f.base;
    f.refuse(
        endpoint,
        f.local(&base.member, base.member.id, AccessLevel::L1),
        StatusCode::FORBIDDEN,
    )
    .await;
    f.refuse(
        endpoint,
        f.local(&base.stranger, base.member.id, AccessLevel::L1),
        StatusCode::NOT_FOUND,
    )
    .await;
    f.refuse(
        endpoint,
        Request {
            actor: &base.owner,
            company_id: base.foreign.id,
            target_id: f.foreign_member.id,
            level: AccessLevel::L1,
        },
        StatusCode::NOT_FOUND,
    )
    .await;
    f.refuse(
        endpoint,
        f.local(&base.owner, f.foreign_member.id, AccessLevel::L1),
        StatusCode::NOT_FOUND,
    )
    .await;
    f.refuse(
        endpoint,
        f.local(&base.owner, base.owner.id, AccessLevel::L1),
        StatusCode::BAD_REQUEST,
    )
    .await;
    let before = f.snapshot().await;
    let response = endpoint
        .invoke(base, f.local(&base.owner, base.member.id, AccessLevel::L1))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let after = f.snapshot().await;
    only_level_changed(&before.local_member, &after.local_member, AccessLevel::L1);
    assert_eq!(before.foreign_member, after.foreign_member);
    assert_eq!(before.local_invite, after.local_invite);
    assert_eq!(before.foreign_invite, after.foreign_invite);
    f.make_admin().await;
    let before = f.snapshot().await;
    let response = endpoint
        .invoke(base, f.local(&base.member, base.member.id, AccessLevel::L2))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    only_level_changed(
        &before.local_member,
        &f.snapshot().await.local_member,
        AccessLevel::L2,
    );
    // The actor now has level 2; the endpoint must use that current cap.
    f.refuse(
        endpoint,
        f.local(&base.member, base.member.id, AccessLevel::L3),
        StatusCode::FORBIDDEN,
    )
    .await;
}

async fn check_invite_endpoint(endpoint: Endpoint) {
    let Some(f) = RouteFixture::new().await else {
        return;
    };
    let base = &f.base;
    f.refuse(
        endpoint,
        f.local(&base.member, f.local_invite.id, AccessLevel::L1),
        StatusCode::FORBIDDEN,
    )
    .await;
    f.refuse(
        endpoint,
        f.local(&base.stranger, f.local_invite.id, AccessLevel::L1),
        StatusCode::NOT_FOUND,
    )
    .await;
    f.refuse(
        endpoint,
        Request {
            actor: &base.owner,
            company_id: base.foreign.id,
            target_id: f.foreign_invite.id,
            level: AccessLevel::L1,
        },
        StatusCode::NOT_FOUND,
    )
    .await;
    f.refuse(
        endpoint,
        f.local(&base.owner, f.foreign_invite.id, AccessLevel::L1),
        StatusCode::NOT_FOUND,
    )
    .await;
    let before = f.snapshot().await;
    let response = endpoint
        .invoke(
            base,
            f.local(&base.owner, f.local_invite.id, AccessLevel::L1),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let after = f.snapshot().await;
    only_level_changed(&before.local_invite, &after.local_invite, AccessLevel::L1);
    assert_eq!(before.local_member, after.local_member);
    assert_eq!(before.foreign_member, after.foreign_member);
    assert_eq!(before.foreign_invite, after.foreign_invite);
    f.make_admin().await;
    let before = f.snapshot().await;
    let response = endpoint
        .invoke(
            base,
            f.local(&base.member, f.local_invite.id, AccessLevel::L2),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    only_level_changed(
        &before.local_invite,
        &f.snapshot().await.local_invite,
        AccessLevel::L2,
    );
    f.refuse(
        endpoint,
        f.local(&base.member, f.local_invite.id, AccessLevel::L5),
        StatusCode::FORBIDDEN,
    )
    .await;
    endpoint
        .invoke(
            base,
            f.local(&base.owner, f.local_invite.id, AccessLevel::L5),
        )
        .await
        .unwrap();
    f.refuse(
        endpoint,
        f.local(&base.member, f.local_invite.id, AccessLevel::PUBLIC),
        StatusCode::FORBIDDEN,
    )
    .await;
}
