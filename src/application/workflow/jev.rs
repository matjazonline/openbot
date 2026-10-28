//! Company TypeSafe resources, independent of agent-selectable model connections.
use super::{WorkflowActor, lease::ClaimedWorkflow, publication::PublishedBundle};
use crate::{
    app_error::{AppError, AppResult},
    domain::workflow::{Binding, CompanyId, ResourceName},
};
use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub const MAX_CONNECTIONS: i64 = 256;
pub const MAX_MODELS: usize = 64;
pub const MAX_SECRET_BYTES: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct JevModel(String);
impl JevModel {
    pub fn parse(value: &str) -> AppResult<Self> {
        super::registry::jev::validate_model(value)
            .map_err(|message| AppError::BadRequest(message.into()))?;
        Ok(Self(value.into()))
    }
    /// Resource contract identifiers cannot have numeric dotted segments. This
    /// one-to-one encoding preserves the exact version without changing TypeName syntax.
    pub fn resource_contract(&self) -> crate::domain::workflow::TypeName {
        crate::domain::workflow::TypeName::parse(self.0.replace('.', "_"))
            .expect("validated Jev model is a valid flat contract name")
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<'de> Deserialize<'de> for JevModel {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(&String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct JevConnection {
    pub company: CompanyId,
    pub id: Uuid,
    pub enabled: bool,
    pub revision: i64,
    pub models: BTreeSet<JevModel>,
    pub ready: bool,
}
/// Secrets deliberately have neither Debug nor Serialize.
pub struct JevConnectionWrite {
    pub enabled: bool,
    pub models: BTreeSet<JevModel>,
    pub secret: SecretString,
}
pub struct JevConnectionUpdate {
    pub expected_revision: i64,
    pub enabled: bool,
    pub models: BTreeSet<JevModel>,
}
pub fn validate_models(models: &BTreeSet<JevModel>) -> AppResult<()> {
    if models.is_empty() || models.len() > MAX_MODELS {
        return Err(AppError::BadRequest(
            "Use between 1 and 64 versioned Jev models".into(),
        ));
    }
    Ok(())
}
pub fn validate_secret(secret: &SecretString) -> AppResult<()> {
    let value = secret.expose_secret();
    if value.is_empty()
        || value.len() > MAX_SECRET_BYTES
        || !value.bytes().all(|b| b.is_ascii_graphic())
    {
        return Err(AppError::BadRequest(
            "TypeSafe bearer key must contain 1..4096 visible ASCII bytes".into(),
        ));
    }
    Ok(())
}
#[async_trait]
pub trait JevConnections: Send + Sync {
    async fn list_jev(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
    ) -> AppResult<Vec<JevConnection>>;
    async fn read_jev(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        id: Uuid,
    ) -> AppResult<JevConnection>;
    async fn create_jev(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        write: JevConnectionWrite,
    ) -> AppResult<JevConnection>;
    async fn update_jev(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        id: Uuid,
        update: JevConnectionUpdate,
    ) -> AppResult<JevConnection>;
    async fn rotate_jev(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        id: Uuid,
        expected_revision: i64,
        secret: SecretString,
    ) -> AppResult<JevConnection>;
}
/// An ephemeral current-use grant. Never persist this or cache it across provider calls.
pub struct JevCredential {
    pub connection_id: Uuid,
    pub model: JevModel,
    pub bearer: SecretString,
}
#[async_trait]
pub trait JevAuthorization: Send + Sync {
    /// Rechecks persisted selection, current actor/resource AND file authority and execution fence.
    /// All authority locks survive every subsequent credential/fence wait in one transaction.
    async fn authorize_jev(&self, claim: &ClaimedWorkflow) -> AppResult<JevCredential>;
}
/// The immutable model requirements across this published bundle and its children.
pub fn required_models(
    bundle: &PublishedBundle,
) -> AppResult<BTreeMap<ResourceName, BTreeSet<JevModel>>> {
    let mut result: BTreeMap<ResourceName, BTreeSet<JevModel>> = BTreeMap::new();
    let mut pending = vec![bundle];
    let mut seen = BTreeSet::new();
    while let Some(current) = pending.pop() {
        let definition = current.compiled().graph().definition();
        if !seen.insert(definition.version_id) {
            continue;
        }
        if seen.len() > 256 {
            return Err(AppError::BadRequest("Too many Jev dependencies".into()));
        }
        for step in definition
            .steps
            .values()
            .filter(|step| step.step_type.as_str() == "jev.evaluate")
        {
            let text = |name| {
                match step.inputs.get(name) {
                    Some(Binding::Literal(value)) => value.as_str(),
                    _ => None,
                }
                .ok_or_else(|| AppError::BadRequest("Invalid frozen Jev selection".into()))
            };
            let slot = ResourceName::parse(text("connection")?)
                .map_err(|_| AppError::BadRequest("Invalid Jev slot".into()))?;
            result
                .entry(slot)
                .or_default()
                .insert(JevModel::parse(text("model")?)?);
        }
        pending.extend(current.children().values().map(AsRef::as_ref));
    }
    Ok(result)
}

#[path = "jev_runtime.rs"]
mod runtime;
pub use runtime::*;

#[path = "jev_assets.rs"]
pub(crate) mod assets;
pub use assets::*;
