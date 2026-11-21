//! JSON shapes shared by the unit-owned resource routes (Plan07): a move target and its result.
use serde::Deserialize;
use uuid::Uuid;

use crate::application::ports::unit_resources::{MovedResource, ResourceUseKind};

#[derive(Debug, Deserialize)]
pub(crate) struct UnitTarget {
    pub unit_id: Uuid,
}

pub(crate) fn moved_json(moved: &MovedResource) -> serde_json::Value {
    serde_json::json!({
        "from_unit_id": moved.from.as_uuid(),
        "unit_id": moved.to.as_uuid(),
        "uses": moved.uses.iter().map(|used| serde_json::json!({
            "kind": match used.kind {
                ResourceUseKind::Agent => "agent",
                ResourceUseKind::Binding => "binding",
            },
            "id": used.id,
            "name": used.name,
        })).collect::<Vec<_>>(),
        "more_uses": moved.more_uses,
    })
}
