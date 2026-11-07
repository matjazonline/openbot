use super::*;
use crate::entities::{
    message::CanonicalMessageId,
    transport::{DeliveryId, DeliveryPartId},
};
use crate::transport::{DeliveryKey, NewDelivery};
use sqlx::{PgConnection, postgres::PgQueryResult};

struct MembershipFixture {
    f: FinalFixture,
    template: NewDelivery,
    receipt: ReplyPublicationReceipt,
}
struct OrdinaryDelivery {
    message: CanonicalMessageId,
    delivery: DeliveryId,
    part: DeliveryPartId,
}

async fn ready() -> MembershipFixture {
    let f = FinalFixture::new("data.map", false).await;
    f.advance().await;
    let claim = claim(&f).await;
    let plan = super::authority::plan(&f, claim).await;
    let template = plan.destinations[0].delivery.clone().unwrap();
    let ReplyPublicationOutcome::Published(receipt) =
        f.p.commit_reply_publication(claim, &plan).await.unwrap()
    else {
        panic!("published")
    };
    MembershipFixture {
        f,
        template,
        receipt,
    }
}

async fn insert_candidate(
    db: &mut PgConnection,
    delivery: &NewDelivery,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query("INSERT INTO message_deliveries (id,company_id,channel_id,message_id,source_binding_id,destination_binding_id, \
        external_destination,correlation_id,transport,purpose,idempotency_key,max_attempts,message_audience) \
        VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)")
        .bind(delivery.id.as_uuid()).bind(delivery.company_id).bind(delivery.channel_id).bind(delivery.message_id.as_uuid())
        .bind(delivery.source_binding_id.as_uuid()).bind(delivery.destination_binding_id.as_uuid())
        .bind(delivery.external_destination.as_ref().map(|destination|destination.as_str()))
        .bind(delivery.correlation_id.as_uuid()).bind(delivery.transport.as_str()).bind(delivery.purpose.as_str())
        .bind(delivery.idempotency_key.as_str()).bind(delivery.max_attempts).bind(delivery.message_audience.as_str())
        .execute(db).await
}

fn candidate(template: &NewDelivery, message: CanonicalMessageId) -> NewDelivery {
    let mut delivery = template.clone();
    delivery.id = DeliveryId::random();
    delivery.message_id = message;
    delivery.idempotency_key =
        DeliveryKey::parse(format!("membership:{}", delivery.id.as_uuid())).unwrap();
    delivery
}

async fn insert_extra_part(
    db: &mut PgConnection,
    delivery: &NewDelivery,
    part: DeliveryPartId,
) -> Result<PgQueryResult, sqlx::Error> {
    let rendered = &delivery.parts[0];
    sqlx::query("INSERT INTO message_delivery_parts(id,company_id,delivery_id,part_index,part_key,payload,content_digest) VALUES($1,$2,$3,49,$4,$5,$6)")
        .bind(part.as_uuid()).bind(delivery.company_id).bind(delivery.id.as_uuid())
        .bind(format!("membership:{}",part.as_uuid())).bind(serde_json::to_value(&rendered.payload).unwrap())
        .bind(rendered.digest.as_str()).execute(db).await
}

async fn ordinary(case: &MembershipFixture) -> OrdinaryDelivery {
    let message = case.f.s.message().await;
    let delivery = candidate(&case.template, message);
    let part = DeliveryPartId::random();
    let mut db = case.f.p.pool().acquire().await.unwrap();
    insert_candidate(&mut db, &delivery).await.unwrap();
    insert_extra_part(&mut db, &delivery, part).await.unwrap();
    OrdinaryDelivery {
        message,
        delivery: delivery.id,
        part,
    }
}

fn rejected<T>(result: Result<T, sqlx::Error>, code: &str) {
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("invalid membership committed"),
    };
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some(code)
    );
}

#[tokio::test]
async fn channel_reply_publication_part_membership_checks_incoming_and_outgoing_owner() {
    let case = ready().await;
    let donor = ordinary(&case).await;
    let final_delivery = case.receipt.delivery.unwrap();
    rejected(
        sqlx::query(
            "UPDATE message_delivery_parts SET delivery_id=$3 WHERE company_id=$1 AND id=$2",
        )
        .bind(case.f.scope.company.as_uuid())
        .bind(donor.part.as_uuid())
        .bind(final_delivery.as_uuid())
        .execute(case.f.p.pool())
        .await,
        "23514",
    );
    rejected(sqlx::query("UPDATE message_delivery_parts SET delivery_id=$3 WHERE company_id=$1 AND delivery_id=$2")
        .bind(case.f.scope.company.as_uuid()).bind(final_delivery.as_uuid()).bind(donor.delivery.as_uuid()).execute(case.f.p.pool()).await,"23514");
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM message_delivery_parts WHERE company_id=$1 AND delivery_id=$2",
    )
    .bind(case.f.scope.company.as_uuid())
    .bind(final_delivery.as_uuid())
    .fetch_one(case.f.p.pool())
    .await
    .unwrap();
    assert_eq!(count, 1);
    assert_eq!(sqlx::query("UPDATE message_delivery_parts SET part_key='ordinary-renamed' WHERE company_id=$1 AND id=$2")
        .bind(case.f.scope.company.as_uuid()).bind(donor.part.as_uuid()).execute(case.f.p.pool()).await.unwrap().rows_affected(),1);
}

#[tokio::test]
async fn channel_reply_publication_canonical_delivery_membership_rejects_extra_insert_and_update() {
    let case = ready().await;
    let donor = ordinary(&case).await;
    let extra = candidate(&case.template, case.receipt.message);
    let mut db = case.f.p.pool().acquire().await.unwrap();
    rejected(insert_candidate(&mut db, &extra).await, "23514");
    drop(db);
    rejected(
        sqlx::query("UPDATE message_deliveries SET message_id=$3 WHERE company_id=$1 AND id=$2")
            .bind(case.f.scope.company.as_uuid())
            .bind(donor.delivery.as_uuid())
            .bind(case.receipt.message.as_uuid())
            .execute(case.f.p.pool())
            .await,
        "23514",
    );
    let second = candidate(&case.template, donor.message);
    insert_candidate(&mut case.f.p.pool().acquire().await.unwrap(), &second)
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM message_deliveries WHERE company_id=$1 AND message_id=$2",
    )
    .bind(case.f.scope.company.as_uuid())
    .bind(case.receipt.message.as_uuid())
    .fetch_one(case.f.p.pool())
    .await
    .unwrap();
    assert_eq!(
        count, 1,
        "ordinary multi-destination delivery does not grant another final effect"
    );
    sqlx::query(
        "UPDATE channel_bindings SET status='paused' WHERE company_id=$1 AND channel_id=$2",
    )
    .bind(case.f.scope.company.as_uuid())
    .bind(case.f.s.channel)
    .execute(case.f.p.pool())
    .await
    .unwrap();
    let claimed = super::delivery::claim_delivery(&case.f, case.receipt).await;
    assert!(matches!(
        crate::transport::DeliveryQueue::begin_part(&case.f.p, &claimed.lease, claimed.parts[0].id)
            .await
            .unwrap(),
        crate::transport::DeliveryOutcome::Cancelled(_)
    ));
}

#[tokio::test]
async fn channel_reply_publication_component_membership_checks_new_tenant_and_message() {
    let case = ready().await;
    let donor = Intents::in_fixture(
        Fixture::in_database(case.f.s.fixture._db.clone(), "membership-donor").await,
    )
    .await;
    sqlx::query("ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable")
        .execute(case.f.p.pool())
        .await
        .unwrap();
    let source = donor.message().await;
    let channel = new_channel(&case.f.s.fixture, "membership-target").await;
    let target_thread = case
        .f
        .p
        .create_thread(channel, "Target", &[])
        .await
        .unwrap()
        .id;
    rejected(sqlx::query("UPDATE thread_messages SET company_id=$3,channel_id=$4,thread_id=$5,message_id=$6 WHERE company_id=$1 AND message_id=$2")
        .bind(donor.fixture.target.company.as_uuid()).bind(source.as_uuid()).bind(case.f.scope.company.as_uuid()).bind(channel)
        .bind(target_thread).bind(case.receipt.message.as_uuid()).execute(case.f.p.pool()).await,"23514");
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM thread_messages WHERE company_id=$1 AND message_id=$2",
    )
    .bind(case.f.scope.company.as_uuid())
    .bind(case.receipt.message.as_uuid())
    .fetch_one(case.f.p.pool())
    .await
    .unwrap();
    assert_eq!(count, 1);
    let normal_thread = donor
        .p()
        .create_thread(donor.channel, "Ordinary", &[])
        .await
        .unwrap()
        .id;
    assert_eq!(
        sqlx::query(
            "UPDATE thread_messages SET thread_id=$3 WHERE company_id=$1 AND message_id=$2"
        )
        .bind(donor.fixture.target.company.as_uuid())
        .bind(source.as_uuid())
        .bind(normal_thread)
        .execute(donor.pool())
        .await
        .unwrap()
        .rows_affected(),
        1
    );
}

#[tokio::test]
async fn channel_reply_publication_competing_membership_and_commit_keeps_exact_effect_set() {
    let f = FinalFixture::new("data.map", false).await;
    f.advance().await;
    let claim = claim(&f).await;
    let plan = super::authority::plan(&f, claim).await;
    let original = plan.destinations[0].delivery.as_ref().unwrap();
    let extra = candidate(original, original.message_id);
    let part = DeliveryPartId::random();
    let mut blocker = f.p.pool().begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(742981)")
        .execute(&mut *blocker)
        .await
        .unwrap();
    sqlx::raw_sql(&format!("CREATE FUNCTION hold_membership_publication() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
        IF NEW.id='{}'::uuid THEN PERFORM pg_advisory_xact_lock(742981); END IF; RETURN NEW; END $$; \
        CREATE TRIGGER hold_membership_publication AFTER INSERT ON message_deliveries FOR EACH ROW EXECUTE FUNCTION hold_membership_publication();",original.id.as_uuid()))
        .execute(f.p.pool()).await.unwrap();
    let mutations = async {
        crate::adapters::persistence::test_support::wait_until_a_backend_is_blocked(f.p.pool())
            .await;
        let mut first = f.p.pool().acquire().await.unwrap();
        let mut second = f.p.pool().acquire().await.unwrap();
        let (delivery_result, part_result) = tokio::join!(
            insert_candidate(&mut first, &extra),
            insert_extra_part(&mut second, original, part)
        );
        rejected(delivery_result, "23503");
        rejected(part_result, "23503");
        blocker.rollback().await.unwrap();
    };
    let (outcome, ()) = tokio::join!(f.p.commit_reply_publication(claim, &plan), mutations);
    assert!(matches!(
        outcome.unwrap(),
        ReplyPublicationOutcome::Published(_)
    ));
    let shape:Value=sqlx::query_scalar("SELECT jsonb_build_object('deliveries',(SELECT count(*) FROM message_deliveries WHERE company_id=$1 AND message_id=$2), \
        'parts',(SELECT count(*) FROM message_delivery_parts WHERE company_id=$1 AND delivery_id=$3))")
        .bind(f.scope.company.as_uuid()).bind(original.message_id.as_uuid()).bind(original.id.as_uuid()).fetch_one(f.p.pool()).await.unwrap();
    assert_eq!(shape, json!({"deliveries":1,"parts":1}));
}
