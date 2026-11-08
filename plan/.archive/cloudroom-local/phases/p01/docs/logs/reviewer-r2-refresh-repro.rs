#![allow(dead_code)]
#[path = "/Users/mac03/dev/openbot/src/domain/local_agents/mod.rs"] mod local_agents;
use local_agents::*;
fn main() {
 let op=LocalAgentOperation::queued(OperationId::new(uuid::Uuid::new_v4()).unwrap(), LocalCommand::RefreshCatalog {}, chrono::Utc::now()+chrono::Duration::minutes(30)).unwrap();
 let mut stored=serde_json::to_value(&op).unwrap();
 stored["outcome"]=serde_json::json!("succeeded");
 stored["dispatch"]=serde_json::json!({"kind":"accepted_locally","sequence":1});
 let failure=serde_json::from_value::<LocalAgentOperation>(stored.clone()).unwrap_err();
 println!("RefreshCatalog success without unrelated native thread: rejected: {}",failure);
 stored["dispatch"]=serde_json::json!({"kind":"native_identity","sequence":1,"thread_id":"fabricated_thread","turn_id":null});
 assert!(serde_json::from_value::<LocalAgentOperation>(stored).is_ok());
 println!("RefreshCatalog success with fabricated unrelated native thread: accepted");
}
