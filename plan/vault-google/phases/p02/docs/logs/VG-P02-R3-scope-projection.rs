#![allow(dead_code)]
mod domain { #[path = "/Users/mac02/.codex/worktrees/1133/openbot/src/domain/integrations/mod.rs"] pub mod integrations; }
mod application { pub mod integrations {
#[path = "/Users/mac02/.codex/worktrees/1133/openbot/src/application/integrations/private.rs"] pub(crate) mod private;
#[path = "/Users/mac02/.codex/worktrees/1133/openbot/src/application/integrations/registry.rs"] pub(crate) mod registry;
#[path = "/Users/mac02/.codex/worktrees/1133/openbot/src/application/integrations/execution.rs"] pub mod execution;
} }
use application::integrations::{private::CredentialAdapter, registry::StaticCredentialAdapter, execution::OperationArguments};
use secrecy::{SecretString, ExposeSecret};

#[path="/Users/mac02/.codex/worktrees/1133/openbot/src/adapters/integrations/google_credentials.rs"] mod google;
fn main() {
use domain::integrations::*;
let adapter=google::GoogleCredentialAdapter{client:ExternalSubject::parse("client").unwrap()};
let mut payload=serde_json::json!({"subject":"subject","client":"client","scopes":["read"],"refresh_token":"fixture","access_token":null,"expires_at":null,"generation":1,"refresh_retry_at":null});
let old=adapter.validate(&SecretString::from(payload.to_string())).unwrap();
payload["scopes"]=serde_json::json!(["read","send"]);
let new=adapter.validate(&SecretString::from(payload.to_string())).unwrap();
assert_eq!(old,new);
let versions=CredentialVersions::INITIAL;
assert_eq!(versions.changed(CredentialChange::Refresh).unwrap().authorization,versions.authorization);
println!("Google adapter accepts changed granted scopes with unchanged validated identity; Refresh transition preserves authorization revision. write_managed checks only this identity, not old scope facts.");
}
