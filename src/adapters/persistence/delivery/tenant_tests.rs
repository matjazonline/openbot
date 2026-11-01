//! fix-app 05 against PostgreSQL: a notice belongs to its company and its recipient's
//! membership, and an attributed delivery speaks through its own channel's interfaces.
//!
//! Notices are parked beyond every claim's horizon unless a test claims them, so a neighbouring
//! test's global claim never takes one.

use super::*;
use crate::{
    adapters::persistence::test_support::{
        wait_until_a_backend_is_blocked, wait_until_backends_are_blocked,
    },
    entities::transport::DeliveryCancellationReason,
    use_cases::company_invite::CompanyInvitePersistence,
};

/// A member of the scope's company with a person principal, as the team page creates one.
struct Member {
    user_id: Uuid,
    principal_id: Uuid,
}

async fn member(persistence: &PostgresPersistence, company_id: Uuid, role: &str) -> Member {
    let user_id = Uuid::new_v4();
    let principal_id = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,username,email,password_hash) VALUES($1,$2,$3,'test')")
        .bind(user_id)
        .bind(format!("member_{}", user_id.simple()))
        .bind(format!("{}@example.test", user_id.simple()))
        .execute(&persistence.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO company_members(id,company_id,user_id,role) VALUES($1,$2,$3,$4)")
        .bind(Uuid::new_v4())
        .bind(company_id)
        .bind(user_id)
        .bind(role)
        .execute(&persistence.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO principals(id,company_id,kind,user_id,display_label) VALUES($1,$2,'person',$3,'member')")
        .bind(principal_id)
        .bind(company_id)
        .bind(user_id)
        .execute(&persistence.pool)
        .await
        .unwrap();
    Member {
        user_id,
        principal_id,
    }
}

/// A delivery-failure notification for `member` with its email queued, written on `db` so a test
/// can hold it uncommitted. The email is parked out of every claim's reach.
async fn notice_on(db: &mut sqlx::PgConnection, scope: &Scope, member: &Member) -> DeliveryId {
    let notice = standalone(
        scope.company.id,
        DeliveryKey::parse(format!("notification:member:{}", Uuid::new_v4())).unwrap(),
    );
    let mut tx = sqlx::Connection::begin(&mut *db).await.unwrap();
    enqueue::insert_standalone_delivery_on(&mut tx, &notice)
        .await
        .unwrap();
    let event_id: Uuid = sqlx::query_scalar(
        "INSERT INTO notification_events (company_id, source_kind, source_id, action_kind, source_generation) \
         VALUES ($1, 'delivery', $2, 'delivery_failure', 1) RETURNING id",
    )
    .bind(scope.company.id)
    .bind(Uuid::new_v4())
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO notifications (company_id, recipient_user_id, recipient_principal_id, event_id, \
           source_kind, source_id, action_kind, source_generation, channel_id, email_delivery_id) \
         SELECT company_id, $2, $3, id, source_kind, source_id, action_kind, source_generation, $4, $5 \
           FROM notification_events WHERE id = $1",
    )
    .bind(event_id)
    .bind(member.user_id)
    .bind(member.principal_id)
    .bind(scope.channel.id)
    .bind(notice.id.as_uuid())
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("UPDATE message_deliveries SET available_at = $2 WHERE id = $1")
        .bind(notice.id.as_uuid())
        .bind(Utc::now() + chrono::Duration::days(3650))
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    notice.id
}

async fn notice(persistence: &PostgresPersistence, scope: &Scope, member: &Member) -> DeliveryId {
    let mut db = persistence.pool.acquire().await.unwrap();
    notice_on(&mut db, scope, member).await
}

async fn cancellation_of(
    persistence: &PostgresPersistence,
    delivery_id: DeliveryId,
) -> Option<DeliveryCancellationReason> {
    let reason: Option<String> =
        sqlx::query_scalar("SELECT cancellation_reason FROM message_deliveries WHERE id = $1")
            .bind(delivery_id.as_uuid())
            .fetch_one(&persistence.pool)
            .await
            .unwrap();
    reason.map(|reason| DeliveryCancellationReason::from_str(&reason).unwrap())
}

fn constraint(error: &sqlx::Error) -> Option<&str> {
    error
        .as_database_error()
        .and_then(|error| error.constraint())
}

/// Deleting a company takes its pending notices and their frozen parts with it: nothing is left
/// for a worker to send to an address the company no longer vouches for.
#[tokio::test]
async fn notification_delivery_tenant_company_deletion_removes_notices_and_parts() {
    let Some(database) = own_database().await else {
        return;
    };
    let persistence = PostgresPersistence::new(database.pool.clone());
    let scope = scope(&persistence).await;
    let recipient = member(&persistence, scope.company.id, "member").await;
    notice(&persistence, &scope, &recipient).await;
    persistence
        .enqueue_standalone_delivery(standalone(
            scope.company.id,
            DeliveryKey::parse(format!("notification:bounce:{}", Uuid::new_v4())).unwrap(),
        ))
        .await
        .unwrap();

    CompanyPersistence::delete(&persistence, scope.company.id)
        .await
        .unwrap();

    for table in [
        "message_deliveries",
        "message_delivery_parts",
        "notifications",
    ] {
        let left: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&persistence.pool)
            .await
            .unwrap();
        assert_eq!(left, 0, "{table} outlived its company");
    }
}

/// A part carries its delivery's company, and only that one.
#[tokio::test]
async fn notification_delivery_tenant_part_company_must_match_its_delivery() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let home = scope(&persistence).await;
    let foreign = scope(&persistence).await;
    let created = persistence
        .enqueue_standalone_delivery(standalone(
            home.company.id,
            DeliveryKey::parse(format!("notification:bounce:{}", Uuid::new_v4())).unwrap(),
        ))
        .await
        .unwrap();
    park(&persistence, created.delivery_id()).await;
    let insert = |company: Option<Uuid>| {
        sqlx::query(
            "INSERT INTO message_delivery_parts (id, company_id, delivery_id, part_index, part_key, payload, content_digest) \
             SELECT gen_random_uuid(), $2, delivery_id, 7, 'email:extra', payload, content_digest \
               FROM message_delivery_parts WHERE delivery_id = $1",
        )
        .bind(created.delivery_id().as_uuid())
        .bind(company)
    };
    let crossed = insert(Some(foreign.company.id))
        .execute(&persistence.pool)
        .await
        .unwrap_err();
    assert_eq!(
        constraint(&crossed),
        Some("message_delivery_parts_delivery_fk"),
        "{crossed}"
    );
    let untenanted = insert(None).execute(&persistence.pool).await.unwrap_err();
    assert_eq!(
        untenanted
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("23502"),
        "{untenanted}"
    );
    for company in [home.company.id, foreign.company.id] {
        CompanyPersistence::delete(&persistence, company)
            .await
            .unwrap();
    }
}

/// Removing a member cancels the notices still owed to them, in the removal's transaction; a
/// notice already in flight keeps its lease and receives only the intent.
#[tokio::test]
async fn notification_delivery_tenant_member_removal_cancels_pending_notices() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let scope = scope(&persistence).await;
    let leaving = member(&persistence, scope.company.id, "member").await;
    let staying = member(&persistence, scope.company.id, "member").await;
    let pending = notice(&persistence, &scope, &leaving).await;
    let in_flight = notice(&persistence, &scope, &leaving).await;
    let kept = notice(&persistence, &scope, &staying).await;
    sqlx::query(
        "UPDATE message_deliveries SET status = 'sending', execution_id = gen_random_uuid(), \
           owner_worker_id = gen_random_uuid(), locked_at = clock_timestamp(), \
           lock_expires_at = clock_timestamp() + interval '1 hour' WHERE id = $1",
    )
    .bind(in_flight.as_uuid())
    .execute(&persistence.pool)
    .await
    .unwrap();

    persistence
        .remove_member(scope.company.id, leaving.user_id)
        .await
        .unwrap();

    assert_eq!(
        status_of(&persistence, pending).await,
        DeliveryStatus::DeadLetter
    );
    assert_eq!(
        cancellation_of(&persistence, pending).await,
        Some(DeliveryCancellationReason::RecipientRevoked)
    );
    assert_eq!(
        part_statuses(&persistence, pending).await,
        vec![DeliveryPartStatus::Dead]
    );
    assert_eq!(
        status_of(&persistence, in_flight).await,
        DeliveryStatus::Sending
    );
    assert_eq!(
        cancellation_of(&persistence, in_flight).await,
        Some(DeliveryCancellationReason::RecipientRevoked)
    );
    assert_eq!(status_of(&persistence, kept).await, DeliveryStatus::Pending);
    assert_eq!(cancellation_of(&persistence, kept).await, None);
    let notifications: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM notifications WHERE company_id = $1 AND recipient_user_id = $2",
    )
    .bind(scope.company.id)
    .bind(leaving.user_id)
    .fetch_one(&persistence.pool)
    .await
    .unwrap();
    assert_eq!(
        notifications, 0,
        "a removed member's notifications leave with them"
    );

    // The owner cannot be removed, and nothing owed to them is cancelled by trying.
    let owner = Member {
        user_id: scope.company.user_id,
        principal_id: sqlx::query_scalar(
            "SELECT id FROM principals WHERE company_id = $1 AND user_id = $2",
        )
        .bind(scope.company.id)
        .bind(scope.company.user_id)
        .fetch_one(&persistence.pool)
        .await
        .unwrap(),
    };
    let owed = notice(&persistence, &scope, &owner).await;
    persistence
        .remove_member(scope.company.id, owner.user_id)
        .await
        .unwrap();
    assert_eq!(cancellation_of(&persistence, owed).await, None);

    CompanyPersistence::delete(&persistence, scope.company.id)
        .await
        .unwrap();
}

/// A notice producer holds the member's principal (`FOR SHARE`, as the notification projection
/// and the decision-reviewer guard do) while it writes a notice. A removal that arrives meanwhile
/// waits for it, and then cancels the notice it committed.
#[tokio::test]
async fn notification_delivery_tenant_removal_waits_for_a_producer_and_cancels_its_notice() {
    let Some(database) = own_database().await else {
        return;
    };
    let persistence = PostgresPersistence::new(database.pool.clone());
    let scope = scope(&persistence).await;
    let leaving = member(&persistence, scope.company.id, "member").await;

    let mut producer = persistence.pool.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM principals AS principal JOIN company_members AS member ON member.company_id = principal.company_id AND member.user_id = principal.user_id WHERE principal.company_id = $1 AND principal.id = $2 FOR SHARE OF principal, member")
        .bind(scope.company.id)
        .bind(leaving.principal_id)
        .fetch_one(&mut *producer)
        .await
        .unwrap();
    let produced = notice_on(&mut producer, &scope, &leaving).await;

    let remover = persistence.clone();
    let company = scope.company.id;
    let user = leaving.user_id;
    let removal = tokio::spawn(async move { remover.remove_member(company, user).await });
    wait_until_a_backend_is_blocked(&persistence.pool).await;
    producer.commit().await.unwrap();
    removal.await.unwrap().unwrap();

    assert_eq!(
        cancellation_of(&persistence, produced).await,
        Some(DeliveryCancellationReason::RecipientRevoked)
    );
    assert_eq!(
        status_of(&persistence, produced).await,
        DeliveryStatus::DeadLetter
    );
}

/// The removal-first half of the race (FIXAPP-P05-BR1-F1). Removal demotes the principal and
/// locks the member, then waits on a notice a third transaction holds. A producer that starts
/// meanwhile takes the producers' lock shape -- the member's principal and member row `FOR SHARE` --
/// and so waits behind removal. Once the notice is released, removal cancels it and deletes the
/// member; the producer then finds no recipient, and the member key refuses a notification written
/// regardless.
#[tokio::test]
async fn notification_delivery_tenant_removal_first_producer_finds_no_member() {
    let Some(database) = own_database().await else {
        return;
    };
    let persistence = PostgresPersistence::new(database.pool.clone());
    let scope = scope(&persistence).await;
    let leaving = member(&persistence, scope.company.id, "member").await;
    let held = notice(&persistence, &scope, &leaving).await;

    let mut holder = persistence.pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM message_deliveries WHERE id = $1 FOR UPDATE")
        .bind(held.as_uuid())
        .fetch_one(&mut *holder)
        .await
        .unwrap();

    let remover = persistence.clone();
    let (company, user) = (scope.company.id, leaving.user_id);
    let removal = tokio::spawn(async move { remover.remove_member(company, user).await });
    wait_until_backends_are_blocked(&persistence.pool, 1).await;

    let producer_pool = persistence.pool.clone();
    let (principal, channel) = (leaving.principal_id, scope.channel.id);
    let producer = tokio::spawn(async move {
        let mut tx = producer_pool.begin().await.unwrap();
        let recipient: Option<Uuid> = sqlx::query_scalar(
            "SELECT principal.id FROM principals AS principal \
               JOIN company_members AS member \
                 ON member.company_id = principal.company_id AND member.user_id = principal.user_id \
              WHERE principal.company_id = $1 AND principal.id = $2 \
                FOR SHARE OF principal, member",
        )
        .bind(company)
        .bind(principal)
        .fetch_optional(&mut *tx)
        .await
        .unwrap();
        // What a producer that ignored the answer would write: the member key refuses it.
        let event_id: Uuid = sqlx::query_scalar(
            "INSERT INTO notification_events (company_id, source_kind, source_id, action_kind, source_generation) \
             VALUES ($1, 'delivery', $2, 'delivery_failure', 1) RETURNING id",
        )
        .bind(company)
        .bind(Uuid::new_v4())
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        let written = sqlx::query(
            "INSERT INTO notifications (company_id, recipient_user_id, event_id, source_kind, \
               source_id, action_kind, source_generation, channel_id) \
             SELECT company_id, $2, id, source_kind, source_id, action_kind, source_generation, $3 \
               FROM notification_events WHERE id = $1",
        )
        .bind(event_id)
        .bind(user)
        .bind(channel)
        .execute(&mut *tx)
        .await;
        (recipient, written)
    });
    wait_until_backends_are_blocked(&persistence.pool, 2).await;
    holder.commit().await.unwrap();

    removal.await.unwrap().unwrap();
    let (recipient, written) = producer.await.unwrap();
    assert_eq!(recipient, None, "the producer sees no current member");
    let refused = written.unwrap_err();
    assert_eq!(
        constraint(&refused),
        Some("notifications_recipient_member_fk"),
        "{refused}"
    );
    assert_eq!(
        status_of(&persistence, held).await,
        DeliveryStatus::DeadLetter
    );
    assert_eq!(
        cancellation_of(&persistence, held).await,
        Some(DeliveryCancellationReason::RecipientRevoked)
    );
    let kind: String = sqlx::query_scalar("SELECT kind FROM principals WHERE id = $1")
        .bind(leaving.principal_id)
        .fetch_one(&persistence.pool)
        .await
        .unwrap();
    assert_eq!(kind, "external");
}

/// A notification's event is one composite key, and its recipient is a member of its company.
#[tokio::test]
async fn tenant_fk_small_notification_event_and_recipient_are_tenant_proven() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let home = scope(&persistence).await;
    let foreign = scope(&persistence).await;
    let recipient = member(&persistence, home.company.id, "member").await;
    let outsider = member(&persistence, foreign.company.id, "member").await;
    let foreign_event: Uuid = sqlx::query_scalar(
        "INSERT INTO notification_events (company_id, source_kind, source_id, action_kind, source_generation) \
         VALUES ($1, 'delivery', $2, 'delivery_failure', 1) RETURNING id",
    )
    .bind(foreign.company.id)
    .bind(Uuid::new_v4())
    .fetch_one(&persistence.pool)
    .await
    .unwrap();
    let insert = |event: Uuid, user: Uuid| {
        sqlx::query(
            "INSERT INTO notifications (company_id, recipient_user_id, event_id, source_kind, \
               source_id, action_kind, source_generation, channel_id) \
             SELECT $1, $2, id, source_kind, source_id, action_kind, source_generation, $3 \
               FROM notification_events WHERE id = $4",
        )
        .bind(home.company.id)
        .bind(user)
        .bind(home.channel.id)
        .bind(event)
    };
    let crossed = insert(foreign_event, recipient.user_id)
        .execute(&persistence.pool)
        .await
        .unwrap_err();
    assert_eq!(
        constraint(&crossed),
        Some("notifications_event_identity_fk"),
        "{crossed}"
    );

    let home_event: Uuid = sqlx::query_scalar(
        "INSERT INTO notification_events (company_id, source_kind, source_id, action_kind, source_generation) \
         VALUES ($1, 'delivery', $2, 'delivery_failure', 1) RETURNING id",
    )
    .bind(home.company.id)
    .bind(Uuid::new_v4())
    .fetch_one(&persistence.pool)
    .await
    .unwrap();
    let stranger = insert(home_event, outsider.user_id)
        .execute(&persistence.pool)
        .await
        .unwrap_err();
    assert_eq!(
        constraint(&stranger),
        Some("notifications_recipient_member_fk"),
        "{stranger}"
    );
    insert(home_event, recipient.user_id)
        .execute(&persistence.pool)
        .await
        .unwrap();
    for company in [home.company.id, foreign.company.id] {
        CompanyPersistence::delete(&persistence, company)
            .await
            .unwrap();
    }
}

/// An attributed delivery reaches one channel: its bindings are that channel's.
#[tokio::test]
async fn channel_path_fk_delivery_cannot_cross_channels() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let scope = scope(&persistence).await;
    let fixture = queue(
        &persistence,
        &scope,
        DeliveryFixtureRequest::new(
            scope.company.id,
            scope.channel.id,
            scope.thread_id,
            "cross-channel",
        ),
    )
    .await;
    park(&persistence, fixture.delivery.id).await;
    let other = ChannelPersistence::create(
        &persistence,
        scope.company.id,
        ChannelWrite {
            name: "Other".into(),
            slug: "other".into(),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let crossed = sqlx::query("UPDATE message_deliveries SET channel_id = $2 WHERE id = $1")
        .bind(fixture.delivery.id.as_uuid())
        .bind(other.id)
        .execute(&persistence.pool)
        .await
        .unwrap_err();
    assert!(
        matches!(
            constraint(&crossed),
            Some(
                "message_deliveries_destination_binding_fk"
                    | "message_deliveries_source_binding_fk"
            )
        ),
        "{crossed}"
    );
    CompanyPersistence::delete(&persistence, scope.company.id)
        .await
        .unwrap();
}

/// Workflow fairness metadata has a parent: deleting the company removes its demand and its poll
/// cursors.
#[tokio::test]
async fn tenant_fk_small_company_deletion_leaves_no_demand_or_poll_rows() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let scope = scope(&persistence).await;
    let worker = Uuid::new_v4();
    sqlx::query("INSERT INTO workflow_dispatch_demand (company_id, worker_id, run_id, execution_id, job_id) VALUES ($1, $2, $3, $4, $5)")
        .bind(scope.company.id)
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .execute(&persistence.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO workflow_poll_workers (worker_id) VALUES ($1)")
        .bind(worker)
        .execute(&persistence.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO workflow_poll_companies (worker_id, company_id) VALUES ($1, $2)")
        .bind(worker)
        .bind(scope.company.id)
        .execute(&persistence.pool)
        .await
        .unwrap();
    let orphan_demand = sqlx::query("INSERT INTO workflow_dispatch_demand (company_id, worker_id, run_id, execution_id, job_id) VALUES ($1, $2, $3, $4, $5)")
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .execute(&persistence.pool)
        .await
        .unwrap_err();
    assert_eq!(
        constraint(&orphan_demand),
        Some("workflow_dispatch_demand_company_fk"),
        "{orphan_demand}"
    );
    let orphan_cursor =
        sqlx::query("INSERT INTO workflow_poll_companies (worker_id, company_id) VALUES ($1, $2)")
            .bind(worker)
            .bind(Uuid::new_v4())
            .execute(&persistence.pool)
            .await
            .unwrap_err();
    assert_eq!(
        constraint(&orphan_cursor),
        Some("workflow_poll_companies_company_fk"),
        "{orphan_cursor}"
    );

    CompanyPersistence::delete(&persistence, scope.company.id)
        .await
        .unwrap();
    for table in ["workflow_dispatch_demand", "workflow_poll_companies"] {
        let left: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) FROM {table} WHERE company_id = $1"
        ))
        .bind(scope.company.id)
        .fetch_one(&persistence.pool)
        .await
        .unwrap();
        assert_eq!(left, 0, "{table} outlived its company");
    }
    sqlx::query("DELETE FROM workflow_poll_workers WHERE worker_id = $1")
        .bind(worker)
        .execute(&persistence.pool)
        .await
        .unwrap();
}
