//! Authorized library lifecycle commands. Durable SQL/CAS/admission integration is
//! implemented in phase03; these ports do not claim production transaction safety.
mod bindings;
mod contracts;
mod definitions;
mod ports;
mod provisioned;
pub use contracts::*;
pub use definitions::compile_bundle;
pub use ports::*;
pub use provisioned::*;

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
    refused_kinds: &'static [&'static str],
}
impl<A, P, D, C> DefinitionService<A, P, D, C> {
    pub fn new(authorization: A, persistence: P, decoder: D, directory: C) -> Self {
        Self {
            authorization,
            persistence,
            decoder,
            directory,
            refused_kinds: &[],
        }
    }

    /// Step kinds this deployment registers but cannot run: a version using one is refused at
    /// validation and publication, so no run can stall on an unsupported step (p08 D1.8).
    pub fn with_refused_kinds(mut self, kinds: &'static [&'static str]) -> Self {
        self.refused_kinds = kinds;
        self
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
