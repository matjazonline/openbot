#[tokio::test]
async fn reviewer_final_part_reparent_must_be_rejected() {
    let f = FinalFixture::new("data.map", false).await;
    f.advance().await;
    let outcome = publisher(&f).publish_claim(claim(&f).await).await.unwrap();
    let ReplyPublicationOutcome::Published(receipt) = outcome else { panic!("{outcome:?}") };
    let final_delivery = receipt.delivery.unwrap().as_uuid();
    let donor = Uuid::new_v4();
    let part = Uuid::new_v4();
    sqlx::query("INSERT INTO message_deliveries (id,company_id,channel_id,message_id,source_binding_id,destination_binding_id,external_destination,correlation_id,transport,purpose,idempotency_key,max_attempts,message_audience) SELECT $3,company_id,channel_id,message_id,source_binding_id,destination_binding_id,external_destination,correlation_id,transport,purpose,$4,max_attempts,message_audience FROM message_deliveries WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company.as_uuid()).bind(final_delivery).bind(donor).bind(format!("reviewer-donor:{donor}"))
        .execute(f.p.pool()).await.unwrap();
    sqlx::query("INSERT INTO message_delivery_parts (id,company_id,delivery_id,part_index,part_key,payload,content_digest) SELECT $4,company_id,$3,49,'reviewer-extra',payload,content_digest FROM message_delivery_parts WHERE company_id=$1 AND delivery_id=$2 LIMIT 1")
        .bind(f.scope.company.as_uuid()).bind(final_delivery).bind(donor).bind(part).execute(f.p.pool()).await.unwrap();
    let moved = sqlx::query("UPDATE message_delivery_parts SET delivery_id=$3 WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company.as_uuid()).bind(part).bind(final_delivery).execute(f.p.pool()).await;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM message_delivery_parts WHERE company_id=$1 AND delivery_id=$2")
        .bind(f.scope.company.as_uuid()).bind(final_delivery).fetch_one(f.p.pool()).await.unwrap();
    eprintln!("reparent result={moved:?}; final part count={count}");
    assert!(moved.is_err(), "published final delivery gained an unapproved part; count={count}");
}
