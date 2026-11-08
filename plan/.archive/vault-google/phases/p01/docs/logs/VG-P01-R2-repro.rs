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
 let payload = SecretString::from(" {\"token\":42}");
 let adapter=StaticCredentialAdapter::bearer();
 assert!(adapter.validate(&payload).is_ok());
 assert_eq!(adapter.injection(&payload).unwrap().value.expose_secret(), " {\"token\":42}");
 println!("F1: unsupported whitespace-prefixed JSON accepted and injected verbatim");
 assert!(OperationArguments::new(serde_json::json!({"headers":{"X-API-Key":"caller-supplied"}})).is_ok());
 println!("F2: registered X-API-Key header accepted as operation arguments");
}
