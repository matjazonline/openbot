//! Exact personal-target authority and retirement through actual delivery readers.
//! Canonical delivery rows use the shared fixture; personal publishing is not claimed here.
use super::*;

async fn join_admin(f: &DirectIntents) -> Uuid {
    use crate::use_cases::company_invite::CompanyInvitePersistence;
    use crate::use_cases::user::UserPersistence;
    let name = format!("delivery_reader_{}", Uuid::new_v4().simple());
    let email = format!("{name}@example.test");
    let p = f.s.p();
    p.create_user(&name, &email, "hash").await.unwrap();
    let account = p.get_by_email(&email).await.unwrap().unwrap();
    // Stamp the external verification result, then exercise production invite acceptance.
    sqlx::query("UPDATE users SET email_verified_at=clock_timestamp() WHERE id=$1")
        .bind(account.id)
        .execute(f.s.pool())
        .await
        .unwrap();
    let command = crate::adapters::persistence::test_support::invite_command(
        p,
        f.s.fixture.target.company.as_uuid(),
        &email,
        crate::entities::company_member::CompanyAccessRole::Admin,
    )
    .await;
    let invite = p.create_invite(command).await.unwrap();
    assert!(
        p.accept_pending_invite(invite.id, account.id, &email)
            .await
            .unwrap()
            .is_some()
    );
    account.id
}

struct PersonalDelivery {
    f: DirectIntents,
    recipient: Uuid,
    manager: Uuid,
    target: AddressTarget,
    private: DeliveryId,
    desk: DeliveryId,
}

async fn personal_delivery() -> PersonalDelivery {
    let f = DirectIntents::new().await;
    let recipient = join_admin(&f).await;
    let manager = join_admin(&f).await;
    let target = AddressTarget::Principal(PrincipalId::new(f.s.fixture.principal(recipient).await));
    let company = f.s.fixture.target.company.as_uuid();
    let desk = delivery_fixture(
        f.s.p(),
        DeliveryFixtureRequest::new(company, f.s.channel, f.s.thread, "private-reader-desk"),
    )
    .await;
    let mut tx = f.s.pool().begin().await.unwrap();
    insert_delivery_on(&mut tx, &desk.delivery).await.unwrap();
    tx.commit().await.unwrap();
    let thread = f.ingest(target).await.thread.unwrap();
    let mut private = None;
    for key in ["private-reader-a", "private-reader-b"] {
        let fixture = delivery_fixture(
            f.s.p(),
            DeliveryFixtureRequest::for_target(company, target, thread.id, key),
        )
        .await;
        let mut tx = f.s.pool().begin().await.unwrap();
        insert_delivery_on(&mut tx, &fixture.delivery)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        private = Some(fixture.delivery.id);
    }
    PersonalDelivery {
        f,
        recipient,
        manager,
        target,
        private: private.unwrap(),
        desk: desk.delivery.id,
    }
}

#[tokio::test]
async fn direct_delivery_private_readers_filter_before_paging_and_recheck_actual_retirement_after_parts()
 {
    let state = Box::pin(personal_delivery()).await;
    let f = &state.f;
    Box::pin(assert_allowed_readers(&state)).await;
    Box::pin(assert_manager_refused(&state)).await;
    Box::pin(retire_during_read(&state)).await;
    assert!(
        f.s.p()
            .get_delivery(read_scope(f, state.recipient), state.private)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        f.s.p()
            .get_delivery(read_scope(f, f.s.owner().user_id()), state.private)
            .await
            .unwrap()
            .unwrap()
            .target,
        state.target
    );
}

async fn assert_allowed_readers(state: &PersonalDelivery) {
    let f = &state.f;
    for viewer in [f.s.owner().user_id(), state.recipient] {
        let found =
            f.s.p()
                .get_delivery(read_scope(f, viewer), state.private)
                .await
                .unwrap()
                .unwrap();
        assert_eq!(found.target, state.target);
        let workspace =
            f.s.p()
                .workspace(
                    read_scope(f, viewer),
                    &DeliveryFilter::new(DeliveryQuery::default()),
                    Some(state.private),
                )
                .await
                .unwrap();
        assert_eq!(workspace.entries.len(), 3);
        assert_eq!(workspace.selected.unwrap().target, state.target);
    }
}

async fn assert_manager_refused(state: &PersonalDelivery) {
    let f = &state.f;
    let scope = read_scope(f, state.manager);
    assert!(
        f.s.p()
            .get_delivery(scope, state.private)
            .await
            .unwrap()
            .is_none()
    );
    let entries =
        f.s.p()
            .list_company_deliveries(
                scope,
                &DeliveryFilter::new(DeliveryQuery {
                    limit: Some(1),
                    ..Default::default()
                }),
            )
            .await
            .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].id, state.desk,
        "two newer hidden personal rows must not consume the page"
    );

    let workspace =
        f.s.p()
            .workspace(
                scope,
                &DeliveryFilter::new(DeliveryQuery {
                    limit: Some(1),
                    ..Default::default()
                }),
                Some(state.private),
            )
            .await
            .unwrap();
    assert_eq!(workspace.entries.len(), 1);
    assert_eq!(workspace.entries[0].id, state.desk);
    assert!(workspace.selected.is_none());
    let foreign_company = crate::use_cases::company::CompanyPersistence::create(
        f.s.p(),
        f.s.owner().user_id(),
        crate::use_cases::company::CompanyWrite {
            name: "Other delivery company".into(),
            slug: format!("delivery-other-{}", Uuid::new_v4().simple()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    // The same viewer can read this personal delivery in its real tenant and owns the
    // second tenant too; refusing here must therefore depend on the tenant boundary.
    let foreign = DeliveryReadScope {
        company_id: foreign_company.id,
        viewer: f.s.owner(),
    };
    assert!(
        f.s.p()
            .get_delivery(foreign, state.private)
            .await
            .unwrap()
            .is_none()
    );
}

async fn retire_during_read(state: &PersonalDelivery) {
    use crate::use_cases::company_invite::CompanyInvitePersistence;
    use sqlx::Connection;
    let f = &state.f;
    let mut observer = sqlx::PgConnection::connect_with(&f.s.pool().connect_options())
        .await
        .unwrap();
    let mut gate = f.s.pool().begin().await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    sqlx::query("LOCK TABLE message_delivery_parts IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *gate)
        .await
        .unwrap();
    let read =
        f.s.p()
            .get_delivery(read_scope(f, state.recipient), state.private);
    let retirement = async {
        tokio::time::timeout(Duration::from_secs(3),async {
            loop {
                let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND $1=ANY(pg_blocking_pids(pid)) AND query LIKE '%message_delivery_parts%')")
                    .bind(blocker).fetch_one(&mut observer).await.unwrap();
                if waiting {break;}
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.expect("real reader loaded the private delivery and reached its parts query");
        f.s.p()
            .remove_member(crate::use_cases::company_invite::RemoveCompanyMember {
                actor_id: f.s.fixture.target.actor.user_id(),
                company_id: f.s.fixture.target.company.as_uuid(),
                member_user_id: state.recipient,
            })
            .await
            .unwrap();
        assert!(
            f.s.p()
                .get_member_by_user_id(f.s.fixture.target.company.as_uuid(), state.recipient)
                .await
                .unwrap()
                .is_none()
        );
        gate.commit().await.unwrap();
    };
    let (read, ()) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(read, retirement)
    })
    .await
    .expect("retirement and the actual reader complete normally");
    assert!(
        read.unwrap().is_none(),
        "authority revoked during the parts read must conceal the loaded private delivery"
    );
}
