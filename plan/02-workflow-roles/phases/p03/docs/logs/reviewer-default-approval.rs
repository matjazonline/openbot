use open_bots::application::workflow::{registry};
use open_bots::adapters::workflow_source::decode;
use open_bots::domain::workflow::{VersionId,StepId,Context,RunMetadata,RunId};
use serde_json::{json,Value};
use std::collections::BTreeMap;
fn main() {
 let mut ex=registry::example("http.request").unwrap();
 let mut source:Value=serde_json::from_str(&ex.source).unwrap();
 source["parameter_schema"]=json!({"type":"object","properties":{"approval":{"type":"object"}}});
 source["steps"]["start"]["with"]["approval"]=json!({"default":{"ref":"/params/approval","value":{"literal":{"reviewers":["reviewer"],"deadline":"2099-01-01T00:00:00Z"}}}});
 ex.source=source.to_string();
 match open_bots::application::workflow::publication::freeze(decode(&ex.source).unwrap(),open_bots::domain::workflow::CompanyId::new(Default::default()),VersionId::new(Default::default()),Default::default(),vec![]) {
  Ok(b)=>println!("FREEZE ACCEPTED; first_role={:?}",b.first_role_reference()), Err(e)=>println!("freeze refused: {e:?}")
 }
 std::fs::write("plan/02-workflow-roles/phases/p03/docs/logs/reviewer-default-approval.json",&ex.source).unwrap();
 match registry::compile(decode(&ex.source).unwrap(), VersionId::new(Default::default()),&ex.facts,&ex.dependencies) {
  Err(e)=>println!("compile refused: {e:?}"),
  Ok(c)=>{
   println!("COMPILE ACCEPTED; roles={:?}",c.role_references());
   let input=json!({}); let params=json!({}); let outputs=BTreeMap::new();
   let ctx=Context{input:&input,params:&params,step_outputs:&outputs,run:RunMetadata{run_id:RunId::new(Default::default()),parent_run_id:None,deadline:None}};
   match c.prepare_step_inputs(&StepId::parse("start").unwrap(),&ctx) {Ok(_)=>println!("PREPARE ACCEPTED"),Err(e)=>println!("prepare refused: {e:?}")}
  }
 }
}
