#![allow(dead_code)]
mod domain { #[path = "/Users/mac02/.codex/worktrees/1133/openbot/src/domain/integrations/mod.rs"] pub mod integrations; }
mod application { pub mod integrations {
#[path = "/Users/mac02/.codex/worktrees/1133/openbot/src/application/integrations/private.rs"] pub(crate) mod private;
#[path = "/Users/mac02/.codex/worktrees/1133/openbot/src/application/integrations/registry.rs"] pub(crate) mod registry;
#[path = "/Users/mac02/.codex/worktrees/1133/openbot/src/application/integrations/execution.rs"] pub mod execution;
} }
use application::integrations::{private::CredentialAdapter, registry::StaticCredentialAdapter, execution::OperationArguments};
use secrecy::{SecretString, ExposeSecret};
fn main() {
 for prefix in [" ", "\t", "\r\n"] {
  for (adapter,key) in [(StaticCredentialAdapter::bearer(),"token"),(StaticCredentialAdapter::api_key_header("X-API-Key").unwrap(),"api_key")] {
   let invalid=SecretString::from(format!("{}{{\"{}\":42}}",prefix,key));
   assert!(adapter.validate(&invalid).is_err());
   assert!(adapter.injection(&invalid).is_err());
   let valid=SecretString::from(format!("{}{{\"{}\":\"value\"}}",prefix,key));
   assert_eq!(adapter.injection(&valid).unwrap().value.expose_secret(),"value");
   let string=SecretString::from(format!("{}\"value\"",prefix));
   assert_eq!(adapter.injection(&string).unwrap().value.expose_secret(),"value");
   assert!(adapter.validate(&SecretString::from(format!("{}[]",prefix))).is_err());
  }
 }
 for name in ["X-API-Key","x-aPi-kEy","X-Custom-Auth"] {
  assert!(OperationArguments::new(serde_json::json!({"headers":{name:"caller-supplied"}})).is_err());
 }
 assert!(OperationArguments::new(serde_json::json!({"headers":{"Accept":"application/json"}})).is_ok());
 assert!(StaticCredentialAdapter::api_key_header("aCcEpT").is_err());
 println!("F1/F2 correction checks pass: bearer/API-key whitespace object/string/array shapes; mixed-case registered/custom credential headers denied; ordinary headers remain valid and cannot become injection destinations.");
}
