use super::*;
use crate::application::agent::*;
use crate::domain::entities::agent_capability::*;

#[path = "autonomous_agent_storage_support.rs"]
mod support;
use support::*;
#[path = "autonomous_agent_storage_native_runs.rs"]
mod native_runs;

#[path = "autonomous_agent_storage_grant_race_tests.rs"]
mod grant_race_tests;
#[path = "autonomous_agent_storage_grant_tests.rs"]
mod grant_tests;
#[path = "autonomous_agent_storage_skill_lifetime_tests.rs"]
mod skill_lifetime_tests;

#[path = "autonomous_agent_storage_draft_tests.rs"]
mod draft_tests;
#[path = "autonomous_agent_storage_manifest_tests.rs"]
mod manifest_tests;
#[path = "autonomous_agent_storage_revision_tests.rs"]
mod revision_tests;
#[path = "autonomous_agent_storage_targets_tests.rs"]
mod targets_tests;
