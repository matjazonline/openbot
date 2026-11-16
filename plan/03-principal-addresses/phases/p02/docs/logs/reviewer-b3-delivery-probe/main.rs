use std::{sync::Arc,time::Duration};
use open_bots::{adapters::{persistence::PostgresPersistence,protocols::email::EmailRenderer},application::{ports::{participant::PrincipalAccessPersistence,thread::qualified_email_identity},settings::ThreadSettings},use_cases::{user::UserPersistence,company::{CompanyPersistence,CompanyWrite},company_invite::CompanyInvitePersistence,thread::{CanonicalMessageIngress,InboundIngestPorts,IngressOrigin,ThreadStores,ThreadUseCases}},entities::{address::AddressTarget},transport::ports::TransportRenderers};
use uuid::Uuid;
use open_bots::{transport::{DeliveryQueue,WorkerId},use_cases::delivery::{DeliveryReader,DeliveryReadScope},application::workflow::WorkflowActor,entities::{delivery::{DeliveryFilter,DeliveryQuery},transport::DeliveryId}};
#[tokio::main]
async fn main(){
 let pool=sqlx::postgres::PgPoolOptions::new().max_connections(8).connect(&std::env::var("TEST_DATABASE_URL").unwrap()).await.unwrap();
 sqlx::query("UPDATE message_deliveries SET available_at=now()+interval '1 day' WHERE status='pending'").execute(&pool).await.unwrap();
 let p=Arc::new(PostgresPersistence::new(pool.clone()));let s=Uuid::new_v4().simple().to_string();
 let owner=p.create_user(&format!("owner_{s}"),&format!("owner-{s}@example.test"),"hash").await.unwrap();
 let member=p.create_user(&format!("member_{s}"),&format!("member-{s}@example.test"),"hash").await.unwrap();
 sqlx::query("UPDATE users SET email_verified_at=now() WHERE id=ANY($1)").bind(vec![owner.id,member.id]).execute(&pool).await.unwrap();
 let company=CompanyPersistence::create(p.as_ref(),owner.id,CompanyWrite{name:"Review audit".into(),slug:format!("audit-{s}"),..Default::default()}).await.unwrap();
 let invite=Uuid::new_v4();sqlx::query("INSERT INTO company_invites(id,company_id,email,role,access_level) VALUES($1,$2,$3,'member',1)").bind(invite).bind(company.id).bind(&member.email).execute(&pool).await.unwrap();
 assert!(p.accept_pending_invite(invite,member.id,&member.email).await.unwrap().is_some());
 let target=AddressTarget::Principal(p.access_context_for_user(company.id,member.id).await.unwrap().unwrap().principal_id.unwrap());
 let service=Arc::new(ThreadUseCases::new(ThreadStores{threads:p.clone(),channels:p.clone(),companies:p.clone(),participants:p.clone()},InboundIngestPorts{committer:p.clone(),correlation:p.clone(),bindings:p.clone(),standalone_deliveries:p.clone()},Arc::new(TransportRenderers::new().register(Arc::new(EmailRenderer::new("direct.test"))).unwrap()),Arc::new(ThreadSettings{mail_domain:"direct.test".into(),max_spam_score:5.0})));
 let result=service.ingest_canonical(CanonicalMessageIngress::new(company.id,target,qualified_email_identity(&member.email).unwrap(),"private body",IngressOrigin::TrustedApplication).with_subject("private subject")).await.unwrap();assert!(result.accepted);let thread=result.thread.unwrap();

 let binding:Uuid=sqlx::query_scalar("SELECT id FROM channel_bindings WHERE company_id=$1 AND principal_id=$2").bind(company.id).bind(target.principal_id().unwrap().as_uuid()).fetch_one(&pool).await.unwrap();
 let message:Uuid=sqlx::query_scalar("SELECT message_id FROM thread_messages WHERE company_id=$1 AND thread_id=$2").bind(company.id).bind(thread.id).fetch_one(&pool).await.unwrap();
 let delivery=Uuid::new_v4();
 let mut listener=sqlx::postgres::PgListener::connect(&std::env::var("TEST_DATABASE_URL").unwrap()).await.unwrap();listener.listen("attention_changed").await.unwrap();
 sqlx::query("INSERT INTO message_deliveries(id,company_id,principal_id,message_id,source_binding_id,destination_binding_id,correlation_id,transport,purpose,idempotency_key,max_attempts,message_audience) VALUES($1,$2,$3,$4,$5,$5,$6,'email','workflow_reply',$7,5,'external_conversation')")
 .bind(delivery).bind(company.id).bind(target.principal_id().unwrap().as_uuid()).bind(message).bind(binding).bind(Uuid::new_v4()).bind(format!("review-{delivery}")).execute(&pool).await.unwrap();
 println!("Native valid canonical direct delivery inserted: {delivery}");
 let wake=tokio::time::timeout(Duration::from_millis(300),listener.recv()).await;println!("Production delivery INSERT attention event: {wake:?}");
 let scope=DeliveryReadScope{company_id:company.id,viewer:WorkflowActor::authenticated(owner.id).unwrap()};
 let read=p.get_delivery(scope,DeliveryId::new(delivery)).await;println!("Actual get_delivery: {read:?}");
 let list=p.list_company_deliveries(scope,&DeliveryFilter::new(DeliveryQuery::default())).await;println!("Actual list_company_deliveries: {list:?}");
 let standalone=Uuid::new_v4();
 sqlx::query("INSERT INTO message_deliveries(id,company_id,external_destination,correlation_id,transport,purpose,idempotency_key,max_attempts) VALUES($1,$2,'other@example.test',$3,'email','notification',$4,5)").bind(standalone).bind(company.id).bind(Uuid::new_v4()).bind(format!("review-{standalone}")).execute(&pool).await.unwrap();
 let claim=p.claim_deliveries(WorkerId::new(Uuid::new_v4()),Duration::from_secs(120),20).await;println!("Actual claim_deliveries: {claim:?}");
 let status:String=sqlx::query_scalar("SELECT status FROM message_deliveries WHERE id=$1").bind(delivery).fetch_one(&pool).await.unwrap();println!("Status after failed claim: {status}");
 let standalone_status:String=sqlx::query_scalar("SELECT status FROM message_deliveries WHERE id=$1").bind(standalone).fetch_one(&pool).await.unwrap();println!("Unrelated standalone status after failed global batch: {standalone_status}");
 sqlx::query("UPDATE message_deliveries SET available_at=now()+interval '1 day' WHERE id=$1").bind(delivery).execute(&pool).await.unwrap();
 let control=p.claim_deliveries(WorkerId::new(Uuid::new_v4()),Duration::from_secs(120),20).await.unwrap();
 assert_eq!(control.len(),1);assert_eq!(control[0].record.id.as_uuid(),standalone);println!("Positive control: unrelated standalone claims successfully when direct row is not eligible");
 assert!(read.is_ok() && list.is_ok() && claim.is_ok(),"Direct canonical delivery must be readable and claimable");
}
