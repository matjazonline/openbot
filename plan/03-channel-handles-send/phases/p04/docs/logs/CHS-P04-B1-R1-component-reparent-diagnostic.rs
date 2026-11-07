#[tokio::test]
async fn reviewer_final_association_cross_tenant_reparent_must_be_rejected() {
    let f = FinalFixture::new("data.map", false).await;
    f.advance().await;
    let ReplyPublicationOutcome::Published(receipt) = publisher(&f).publish_claim(claim(&f).await).await.unwrap() else { panic!("published") };
    let donor = Intents::in_fixture(Fixture::in_database(f.s.fixture._db.clone(), "reviewer-component-donor").await).await;
    let source = donor.message().await;
    let new_thread = Uuid::new_v4();
    sqlx::query("INSERT INTO threads(id,company_id,channel_id,subject) VALUES($1,$2,$3,'Review extra association')")
        .bind(new_thread).bind(f.scope.company.as_uuid()).bind(f.s.channel).execute(f.p.pool()).await.unwrap();
    let moved = sqlx::query("UPDATE thread_messages SET company_id=$3,channel_id=$4,thread_id=$5,message_id=$6 WHERE company_id=$1 AND message_id=$2")
        .bind(donor.fixture.target.company.as_uuid()).bind(source.as_uuid()).bind(f.scope.company.as_uuid()).bind(f.s.channel).bind(new_thread).bind(receipt.message.as_uuid())
        .execute(f.p.pool()).await;
    let count:i64 = sqlx::query_scalar("SELECT count(*) FROM thread_messages WHERE company_id=$1 AND message_id=$2")
        .bind(f.scope.company.as_uuid()).bind(receipt.message.as_uuid()).fetch_one(f.p.pool()).await.unwrap();
    eprintln!("cross-tenant association move={moved:?}; final association count={count}");
    assert!(moved.is_err(), "published final message gained an unapproved association; count={count}");
}
