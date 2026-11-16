use std::{sync::Arc,time::Duration};
use open_bots::{adapters::{persistence::PostgresPersistence,protocols::email::EmailRenderer},application::{ports::{participant::PrincipalAccessPersistence,thread::qualified_email_identity},settings::ThreadSettings},use_cases::{user::UserPersistence,company::{CompanyPersistence,CompanyWrite},company_invite::CompanyInvitePersistence,thread::{CanonicalMessageIngress,InboundIngestPorts,IngressOrigin,InboxReadScope,ThreadReadScope,ThreadStores,ThreadUseCases}},entities::{address::AddressTarget,user::Viewer},transport::ports::TransportRenderers};
use uuid::Uuid;
#[tokio::main]
async fn main(){
 let pool=sqlx::postgres::PgPoolOptions::new().max_connections(8).connect(&std::env::var("TEST_DATABASE_URL").unwrap()).await.unwrap();
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
 let association:Uuid=sqlx::query_scalar("SELECT id FROM thread_messages WHERE company_id=$1 AND thread_id=$2").bind(company.id).bind(thread.id).fetch_one(&pool).await.unwrap();
 let viewer=Viewer{user_id:member.id,email:member.email.into(),email_verified_at:None};
 let scope=ThreadReadScope{inbox:InboxReadScope{company_id:company.id,target},thread_id:thread.id};
 let mut barrier=pool.begin().await.unwrap();sqlx::query("LOCK TABLE external_messages IN ACCESS EXCLUSIVE MODE").execute(&mut *barrier).await.unwrap();
 let call=service.clone();let v=viewer.clone();let task=tokio::spawn(async move {call.get_message_audit(&v,scope,association).await});
 tokio::time::timeout(Duration::from_secs(5),async {loop{let blocked:i64=sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE '%SELECT association.id,%'").fetch_one(&pool).await.unwrap();if blocked>0{break;}tokio::time::sleep(Duration::from_millis(10)).await;}}).await.expect("audit read reached barrier after initial authorization");
 p.remove_member(company.id,viewer.user_id).await.unwrap();assert!(p.access_context_for_user(company.id,viewer.user_id).await.unwrap().is_none());println!("membership removed while actual audit payload query blocked");
 barrier.commit().await.unwrap();let audit=tokio::time::timeout(Duration::from_secs(10),task).await.unwrap().unwrap();println!("audit after removal: {audit:?}");
 let history=service.get_thread_history(&viewer,scope).await;println!("history after removal: {history:?}");assert!(history.is_err());
 assert!(audit.is_err() || audit.unwrap().is_none(),"SECURITY: former member received direct diagnostics after removal committed during payload read");
}
