use open_bots::{adapters::{persistence::PostgresPersistence,protocols::email::EmailIdentity},application::ports::participant::PrincipalAccessPersistence,use_cases::{user::{UserPersistence,ProfileUpdate},company::{CompanyPersistence,CompanyWrite},company_invite::CompanyInvitePersistence},entities::value_objects::EmailAddress};
use uuid::Uuid;
use open_bots::application::ports::channel::{ChannelPersistence,ChannelWrite};
async fn join(p:&PostgresPersistence,pool:&sqlx::PgPool,company:Uuid,user:Uuid,email:&str){
 let invite=Uuid::new_v4();
 sqlx::query("INSERT INTO company_invites(id,company_id,email,role,access_level) VALUES($1,$2,$3,'member',1)").bind(invite).bind(company).bind(email).execute(pool).await.unwrap();
 assert!(p.accept_pending_invite(invite,user,email).await.unwrap().is_some());
}
#[tokio::main]
async fn main(){
 let pool=sqlx::postgres::PgPoolOptions::new().max_connections(3).connect(&std::env::var("TEST_DATABASE_URL").unwrap()).await.unwrap();
 let p=PostgresPersistence::new(pool.clone());
 let s=Uuid::new_v4().simple().to_string();let owner_email=format!("owner-{s}@example.test");let old=format!("old-{s}@example.test");let new=EmailAddress::from(format!("new-{s}@example.test"));let member_name=format!("member_{s}");
 p.create_user(&format!("owner_{s}"),&owner_email,"hash").await.unwrap();let owner=p.get_by_email(&owner_email).await.unwrap().unwrap();
 let company=CompanyPersistence::create(&p,owner.id,CompanyWrite{name:"Review".into(),slug:format!("review-{s}"),..Default::default()}).await.unwrap();
 p.create_user(&member_name,&old,"hash").await.unwrap();let member=p.get_by_email(&old).await.unwrap().unwrap();
 sqlx::query("UPDATE users SET email_verified_at=now() WHERE id=$1").bind(member.id).execute(&pool).await.unwrap();
 join(&p,&pool,company.id,member.id,&old).await;
 let original:Uuid=sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND user_id=$2").bind(company.id).bind(member.id).fetch_one(&pool).await.unwrap();
 p.remove_member(company.id,member.id).await.unwrap();
 p.update_profile(member.id,ProfileUpdate{username:&member_name,email:&new,avatar_url:None}).await.unwrap().unwrap();
 sqlx::query("UPDATE users SET email_verified_at=now() WHERE id=$1").bind(member.id).execute(&pool).await.unwrap();
 let desk=ChannelPersistence::create(&p,company.id,ChannelWrite{name:"Granted outsider".into(),slug:"granted-outsider".into(),participant_emails:Some(vec![new.to_string()]),..Default::default()}).await.unwrap();
 let granted_before:Vec<Uuid>=sqlx::query_scalar("SELECT principal_id FROM channel_principal_grants WHERE company_id=$1 AND channel_id=$2").bind(company.id).bind(desk.id).fetch_all(&pool).await.unwrap();
 join(&p,&pool,company.id,member.id,new.as_str()).await;
 let old_identity=EmailIdentity::parse(EmailAddress::from(old)).unwrap().qualify_default();
 let current=p.access_context_for_identity(company.id,&old_identity).await.unwrap();
 println!("original_principal={original}; old_email_context_after_rejoin={current:?}");
 let viewer=p.access_context_for_user(company.id,member.id).await.unwrap().unwrap();
 let desk=ChannelPersistence::get_by_id(&p,desk.id).await.unwrap().unwrap();
 println!("new_email_allowlist_principals_before={granted_before:?}; restored_person={:?}; new_email_desk_view_after_rejoin={}",viewer.principal_id,desk.viewer_access(viewer));
 assert!(!current.membership.is_team(),"SECURITY: retired old email regained team authority when the lifetime principal was restored");
 assert!(desk.viewer_access(viewer),"REGRESSION: new email adoption lost its existing desk grant");
}
