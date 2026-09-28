//! Authorized library lifecycle commands. Durable SQL/CAS/admission integration is
//! implemented in phase03; these ports do not claim production transaction safety.
mod bindings;
mod contracts;
mod definitions;
mod ports;
pub use contracts::*;
pub use ports::*;

use super::IdempotencyKey;
use super::binding::{BindingConfiguration, ConfiguredBinding, ResourceDirectory};
use super::publication::PublishedBundle;
use super::templates::CompanyWorkflowDraft;
use super::{
    RelatedAssociation, WorkflowActor, WorkflowAuthorization, WorkflowOperation, compiler,
    publication,
};
use crate::app_error::{AppError, AppResult};
use crate::domain::workflow::{
    BindingRevision, BindingStateRevision, CompanyId, DraftRevision, ResourceName,
    RuntimeResourceId, VersionId, WorkflowBindingId, WorkflowId,
};
use std::sync::Arc;

pub struct DefinitionService<A, P, D, C> {
    authorization: A,
    persistence: P,
    decoder: D,
    directory: C,
}
impl<A, P, D, C> DefinitionService<A, P, D, C> {
    pub fn new(authorization: A, persistence: P, decoder: D, directory: C) -> Self {
        Self {
            authorization,
            persistence,
            decoder,
            directory,
        }
    }
}

pub struct BindingService<A, P, V, R> {
    authorization: A,
    persistence: P,
    versions: V,
    resources: R,
}
impl<A, P, V, R> BindingService<A, P, V, R> {
    pub fn new(authorization: A, persistence: P, versions: V, resources: R) -> Self {
        Self {
            authorization,
            persistence,
            versions,
            resources,
        }
    }
}

#[cfg(test)]
mod tests;
