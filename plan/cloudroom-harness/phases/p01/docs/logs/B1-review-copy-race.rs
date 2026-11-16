use open_bots::use_cases::user::UserPersistence;
use open_bots::{adapters::persistence::PostgresPersistence, entities::{creation::CreationProvenance,skill_bundle::SkillFile},use_cases::{company::{CompanyPersistence,CompanyWrite},skill::{SkillManagementPersistence,SkillWrite}}};
use std::time::Duration;
fn write(slug:&str,version:&str)->SkillWrite {SkillWrite{slug:slug.into(),name:"Race probe".into(),description:"Race probe".into(),instructions:format!("Instructions {version}"),files:Some(vec![SkillFile::from_bytes("scripts/check.sh".into(),version.as_bytes(),true).unwrap()]),required_tools:None,created_by:Some(CreationProvenance::system())}}
fn main(){tokio::runtime::Runtime::new().unwrap().block_on(run());}
async fn run(){
 let url="postgresql://mac02@127.0.0.1:63451/openbot_cloudroom_harness_t1gfnhz1";
 let pool=sqlx::postgres::PgPoolOptions::new().max_connections(4).connect(url).await.unwrap();
 let p=PostgresPersistence::new(pool.clone());
 let label=format!("review-copy-{}",uuid::Uuid::new_v4().simple());
 let user=p.create_user(&label,&format!("{label}@example.test"),"unused").await.unwrap();
 let company=CompanyPersistence::create(&p,user.id,CompanyWrite{name:label.clone(),slug:label.clone(),..Default::default()}).await.unwrap();
 let source=p.create_library(write(&label,"A")).await.unwrap();
 let mut blocker=pool.begin().await.unwrap();
 sqlx::query("INSERT INTO skills(id,company_id,slug,name,description,instructions,created_by,files) SELECT $1,$2,slug,name,description,instructions,created_by,files FROM skills WHERE id=$3")
  .bind(uuid::Uuid::new_v4()).bind(company.id).bind(source.id).execute(&mut *blocker).await.unwrap();
 let copy_pool=sqlx::postgres::PgPoolOptions::new().max_connections(1).connect(&format!("{url}?application_name=ch_b1_review_copy")).await.unwrap();
 let cp=PostgresPersistence::new(copy_pool.clone()); let sid=source.id;let cid=company.id;
 let worker=tokio::spawn(async move {cp.copy_library_to_company(cid,sid,CreationProvenance::system()).await});
 let mut waiting=false;
 for _ in 0..200 {
  waiting=sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE application_name='ch_b1_review_copy' AND wait_event_type='Lock')").fetch_one(&pool).await.unwrap();
  if waiting{break;} tokio::time::sleep(Duration::from_millis(10)).await;
 }
 assert!(waiting,"copy must block on the first conflicting INSERT after reading source A");
 p.update_library(source.id,write(&label,"B")).await.unwrap();
 blocker.commit().await.unwrap();
 let copied=worker.await.unwrap().unwrap();
 println!("Actual production copy_library_to_company after concurrent source update: instructions={:?}; asset bytes={:?}; suffix={}",copied.instructions,String::from_utf8(copied.files[0].bytes().unwrap()).unwrap(),copied.slug.as_str().ends_with("-2"));
 assert_eq!(copied.instructions,"Instructions A"); assert_eq!(copied.files[0].bytes().unwrap(),b"B");
 sqlx::query("DELETE FROM companies WHERE id=$1").bind(company.id).execute(&pool).await.unwrap();
 sqlx::query("DELETE FROM skills WHERE id=$1").bind(source.id).execute(&pool).await.unwrap();
 sqlx::query("DELETE FROM users WHERE id=$1").bind(user.id).execute(&pool).await.unwrap();
 copy_pool.close().await;pool.close().await;
 println!("Confirmed mixed source revisions; reviewer-created UUID rows removed.");
}
