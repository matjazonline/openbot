#[tokio::test]
async fn reviewer_final_delivery_alias_must_not_bypass_revocation() {
    let f = FinalFixture::new("data.map", false).await;
    f.advance().await;
    let outcome = publisher(&f).publish_claim(claim(&f).await).await.unwrap();
    let ReplyPublicationOutcome::Published(receipt) = outcome else { panic!("{outcome:?}") };
    let final_delivery = receipt.delivery.unwrap().as_uuid();
    let donor = Uuid::new_v4();
    sqlx::query("INSERT INTO message_deliveries (id,company_id,channel_id,message_id,source_binding_id,destination_binding_id,external_destination,correlation_id,transport,purpose,idempotency_key,max_attempts,message_audience) SELECT $3,company_id,channel_id,message_id,source_binding_id,destination_binding_id,external_destination,correlation_id,transport,purpose,$4,max_attempts,message_audience FROM message_deliveries WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company.as_uuid()).bind(final_delivery).bind(donor).bind(format!("reviewer-alias:{donor}"))
        .execute(f.p.pool()).await.unwrap();
    sqlx::query("INSERT INTO message_delivery_parts (id,company_id,delivery_id,part_index,part_key,payload,content_digest) SELECT gen_random_uuid(),company_id,$3,part_index,part_key,payload,content_digest FROM message_delivery_parts WHERE company_id=$1 AND delivery_id=$2")
        .bind(f.scope.company.as_uuid()).bind(final_delivery).bind(donor).execute(f.p.pool()).await.unwrap();
    sqlx::query("UPDATE channel_bindings SET status='paused' WHERE company_id=$1 AND channel_id=$2")
        .bind(f.scope.company.as_uuid()).bind(f.s.channel).execute(f.p.pool()).await.unwrap();
    let claimed = f.p.claim_deliveries(crate::transport::WorkerId::random(), Duration::from_secs(120),64).await.unwrap();
    let original = claimed.iter().find(|c| c.record.id.as_uuid()==final_delivery).unwrap();
    let alias = claimed.iter().find(|c| c.record.id.as_uuid()==donor).unwrap();
    let original_entry = f.p.begin_part(&original.lease,original.parts[0].id).await.unwrap();
    let alias_entry = f.p.begin_part(&alias.lease,alias.parts[0].id).await.unwrap();
    eprintln!("original provider entry={original_entry:?}; alias provider entry={alias_entry:?}");
    assert!(matches!(original_entry,DeliveryOutcome::Cancelled(_)));
    assert!(!matches!(alias_entry,DeliveryOutcome::Applied(_)),"unowned alias of final canonical message bypasses binding revocation");
}
