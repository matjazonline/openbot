//! Stable execution identities shared by continuation and transactional effect ports.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! identity {
    ($name:ident, $inner:ty) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub $inner);
    };
}
identity!(RunId, Uuid);
identity!(InvocationId, Uuid);
identity!(CheckpointRevision, u64);
identity!(ModelRequestId, Uuid);

/// Trusted host context, never part of a model-supplied tool argument schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvocationRef {
    pub run_id: RunId,
    pub invocation_id: InvocationId,
    pub expected_revision: CheckpointRevision,
}
