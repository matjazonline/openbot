use super::{FkExpectation, PostgresPersistence, finish_fk_probe, foreign_company};
use crate::domain::workflow::CompanyId;
use uuid::Uuid;

const IDENTITY: &str = "message_participants_identity_fk";

async fn referenced_identity(p: &PostgresPersistence, company: CompanyId, message: Uuid) -> Uuid {
    let principal = Uuid::new_v4();
    let identity = Uuid::new_v4();
    let mut tx = p.pool().begin().await.unwrap();
    sqlx::query("INSERT INTO principals(id,company_id,kind,display_label) VALUES($1,$2,'external','Identity FK control')")
        .bind(principal).bind(company.as_uuid()).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO participant_identities(id,company_id,principal_id,transport,namespace,subject,status,claim_metadata,provenance) VALUES($1,$2,$3,'email','email',$4,'observed','{\"kind\":\"observation\",\"version\":1}','transport_ingress')")
        .bind(identity).bind(company.as_uuid()).bind(principal)
        .bind(format!("fk-{}@example.test", identity.simple())).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO message_participants(company_id,message_id,participant_identity_id,kind,position) SELECT $1,$2,$3,'to',coalesce(max(position),-1)+1 FROM message_participants WHERE company_id=$1 AND message_id=$2 AND kind='to'")
        .bind(company.as_uuid()).bind(message).bind(identity).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    identity
}

async fn probe(
    p: &PostgresPersistence,
    company: CompanyId,
    message: Uuid,
    identity: Uuid,
    expected: FkExpectation,
) {
    let mut tx = p.pool().begin().await.unwrap();
    sqlx::query("INSERT INTO message_participants(company_id,message_id,participant_identity_id,kind,position) VALUES($1,$2,$3,'cc',2147483647)")
        .bind(company.as_uuid()).bind(message).bind(identity).execute(&mut *tx).await.unwrap();
    finish_fk_probe(tx, expected).await;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM message_participants WHERE company_id=$1 AND message_id=$2 AND kind='cc' AND position=2147483647")
        .bind(company.as_uuid()).bind(message).fetch_one(p.pool()).await.unwrap();
    assert_eq!(count, 0);
}

pub(crate) async fn assert_relationships(p: &PostgresPersistence, company: CompanyId, owner: Uuid) {
    let message: Uuid = sqlx::query_scalar("SELECT message_id FROM message_participants WHERE company_id=$1 ORDER BY message_id,position LIMIT 1")
        .bind(company.as_uuid()).fetch_one(p.pool()).await.unwrap();
    let identity = referenced_identity(p, company, message).await;
    let foreign = foreign_company(p, owner).await;
    let foreign_identity: Uuid = sqlx::query_scalar(
        "SELECT id FROM participant_identities WHERE company_id=$1 ORDER BY id LIMIT 1",
    )
    .bind(foreign.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    probe(p, company, message, identity, FkExpectation::Valid).await;
    probe(
        p,
        company,
        message,
        Uuid::new_v4(),
        FkExpectation::Refused(IDENTITY),
    )
    .await;
    probe(
        p,
        company,
        message,
        foreign_identity,
        FkExpectation::Refused(IDENTITY),
    )
    .await;
    let mut tx = p.pool().begin().await.unwrap();
    sqlx::query("DELETE FROM participant_identities WHERE company_id=$1 AND id=$2")
        .bind(company.as_uuid())
        .bind(identity)
        .execute(&mut *tx)
        .await
        .unwrap();
    finish_fk_probe(tx, FkExpectation::Refused(IDENTITY)).await;
    let retained: Uuid =
        sqlx::query_scalar("SELECT id FROM participant_identities WHERE company_id=$1 AND id=$2")
            .bind(company.as_uuid())
            .bind(identity)
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(
        retained, identity,
        "referenced identity deletion rolled back"
    );
}
