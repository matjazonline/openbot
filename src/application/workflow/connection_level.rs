//! Actor-bearing connection classification write. Ordinary connection settings cannot set it.
use crate::application::workflow::WorkflowActor;
use crate::domain::workflow::CompanyId;
use crate::entities::access_level::AccessLevel;
use uuid::Uuid;
#[derive(Clone, Copy)]
pub struct ConnectionLevelCommand {
    pub company: CompanyId,
    pub actor: WorkflowActor,
    pub connection: Uuid,
    pub expected_revision: i64,
    pub access_level: AccessLevel,
}
