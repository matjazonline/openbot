//! Client grants and request narrowing (decisions D3). Requests may narrow a grant by naming a
//! subset of namespaces or a lower ceiling; asking for more is refused, never clamped.
use super::access_level::AccessLevel;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

/// Every tenant-scoped client operation. Grants list a subset per namespace.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Save,
    Search,
    Timeline,
    Fetch,
    Correct,
    Delete,
    Context,
    Sources,
    Sessions,
    Generate,
    GenerationStatus,
    Purge,
}

impl Operation {
    pub const ALL: [Self; 12] = [
        Self::Save,
        Self::Search,
        Self::Timeline,
        Self::Fetch,
        Self::Correct,
        Self::Delete,
        Self::Context,
        Self::Sources,
        Self::Sessions,
        Self::Generate,
        Self::GenerationStatus,
        Self::Purge,
    ];
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Save => "save",
            Self::Search => "search",
            Self::Timeline => "timeline",
            Self::Fetch => "fetch",
            Self::Correct => "correct",
            Self::Delete => "delete",
            Self::Context => "context",
            Self::Sources => "sources",
            Self::Sessions => "sessions",
            Self::Generate => "generate",
            Self::GenerationStatus => "generation_status",
            Self::Purge => "purge",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|op| op.as_str() == value)
    }
    /// Read operations name a `read_ceiling` that must be within `read_max`: they can only see
    /// records at or below it. Correct and delete read: an id above the ceiling is `not_found`.
    pub const fn reads(self) -> bool {
        matches!(
            self,
            Self::Search
                | Self::Timeline
                | Self::Fetch
                | Self::Context
                | Self::GenerationStatus
                | Self::Correct
                | Self::Delete
        )
    }
    /// Write operations change state and need `write_max`; new content is labelled at most
    /// `write_max`. Purge is a write but not a read: it removes every level in the namespace,
    /// so granting `purge` grants that authority.
    pub const fn writes(self) -> bool {
        matches!(
            self,
            Self::Save
                | Self::Correct
                | Self::Delete
                | Self::Sources
                | Self::Sessions
                | Self::Generate
                | Self::Purge
        )
    }
}

/// One namespace's grant for one credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespaceGrant {
    pub namespace_id: Uuid,
    pub namespace: String,
    pub read_max: Option<AccessLevel>,
    pub write_max: Option<AccessLevel>,
    pub operations: BTreeSet<Operation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Denial {
    /// Ungranted, unknown and deleted namespaces are indistinguishable.
    #[error("the namespace is not granted to this credential")]
    NamespaceNotGranted,
    #[error("the operation is not granted on this namespace")]
    OperationNotGranted,
    #[error("the requested ceiling exceeds the namespace read grant")]
    CeilingExceedsGrant,
    #[error("a read must name its ceiling")]
    MissingReadCeiling,
    #[error("the namespace grant has no write authority")]
    WriteNotGranted,
    #[error("no namespace was named")]
    NoNamespace,
    #[error("too many namespaces were named")]
    TooManyNamespaces,
}

pub use super::bounds::MAX_NAMESPACES_PER_REQUEST;

/// The authorized scope of one request after narrowing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedScope {
    pub namespace_ids: Vec<Uuid>,
    /// Exactly the requested ceiling, for reads; `None` for pure writes.
    pub read_ceiling: Option<AccessLevel>,
    /// The narrowest write grant across the named namespaces, for writes.
    pub write_max: Option<AccessLevel>,
}

/// Authorize `operation` over `requested` namespaces with an optional requested ceiling.
pub fn authorize(
    grants: &[NamespaceGrant],
    operation: Operation,
    requested: &[String],
    read_ceiling: Option<AccessLevel>,
) -> Result<AuthorizedScope, Denial> {
    if requested.is_empty() {
        return Err(Denial::NoNamespace);
    }
    if requested.len() > MAX_NAMESPACES_PER_REQUEST {
        return Err(Denial::TooManyNamespaces);
    }
    if operation.reads() && read_ceiling.is_none() {
        return Err(Denial::MissingReadCeiling);
    }
    let unique: BTreeSet<&String> = requested.iter().collect();
    let mut namespace_ids = Vec::with_capacity(unique.len());
    let mut write_max: Option<AccessLevel> = None;
    for (index, name) in unique.into_iter().enumerate() {
        let grant = grants
            .iter()
            .find(|grant| &grant.namespace == name)
            .ok_or(Denial::NamespaceNotGranted)?;
        if !grant.operations.contains(&operation) {
            return Err(Denial::OperationNotGranted);
        }
        if let Some(ceiling) = read_ceiling {
            match grant.read_max {
                Some(read_max) if ceiling <= read_max => {}
                _ => return Err(Denial::CeilingExceedsGrant),
            }
        }
        write_max = if index == 0 {
            grant.write_max
        } else {
            match (write_max, grant.write_max) {
                (Some(left), Some(right)) => Some(left.min(right)),
                _ => None,
            }
        };
        if operation.writes() && grant.write_max.is_none() {
            return Err(Denial::WriteNotGranted);
        }
        namespace_ids.push(grant.namespace_id);
    }
    Ok(AuthorizedScope {
        namespace_ids,
        read_ceiling,
        write_max,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(value: i64) -> AccessLevel {
        AccessLevel::new(value).unwrap()
    }
    fn grant(
        name: &str,
        read: Option<i64>,
        write: Option<i64>,
        ops: &[Operation],
    ) -> NamespaceGrant {
        NamespaceGrant {
            namespace_id: Uuid::new_v4(),
            namespace: name.into(),
            read_max: read.map(level),
            write_max: write.map(level),
            operations: ops.iter().copied().collect(),
        }
    }

    #[test]
    fn memory_authority_request_fields_narrow_but_never_widen() {
        let grants = vec![
            grant("a", Some(6), Some(6), &[Operation::Search, Operation::Save]),
            grant("b", Some(4), Some(2), &[Operation::Search, Operation::Save]),
        ];
        let names = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        // A subset and a lower ceiling are accepted exactly as requested.
        let scope = authorize(&grants, Operation::Search, &names(&["a"]), Some(level(3))).unwrap();
        assert_eq!(scope.read_ceiling, Some(level(3)));
        assert_eq!(scope.namespace_ids, vec![grants[0].namespace_id]);
        // A ceiling above any named namespace's grant is refused, not clamped.
        assert_eq!(
            authorize(
                &grants,
                Operation::Search,
                &names(&["a", "b"]),
                Some(level(5))
            ),
            Err(Denial::CeilingExceedsGrant)
        );
        assert!(
            authorize(
                &grants,
                Operation::Search,
                &names(&["a", "b"]),
                Some(level(4))
            )
            .is_ok()
        );
        // Ungranted and unknown namespaces are the same denial.
        assert_eq!(
            authorize(&grants, Operation::Search, &names(&["c"]), Some(level(0))),
            Err(Denial::NamespaceNotGranted)
        );
        // Reads must name a ceiling; operations outside the grant are refused.
        assert_eq!(
            authorize(&grants, Operation::Search, &names(&["a"]), None),
            Err(Denial::MissingReadCeiling)
        );
        assert_eq!(
            authorize(&grants, Operation::Timeline, &names(&["a"]), Some(level(0))),
            Err(Denial::OperationNotGranted)
        );
        // The write bound across namespaces is the narrowest one.
        let scope = authorize(&grants, Operation::Save, &names(&["a", "b"]), None).unwrap();
        assert_eq!(scope.write_max, Some(level(2)));
    }

    #[test]
    fn memory_authority_namespace_list_bounds() {
        let grants = vec![grant("a", Some(9), None, &[Operation::Search])];
        assert_eq!(
            authorize(&grants, Operation::Search, &[], Some(level(0))),
            Err(Denial::NoNamespace)
        );
        let many: Vec<String> = (0..=MAX_NAMESPACES_PER_REQUEST)
            .map(|i| i.to_string())
            .collect();
        assert_eq!(
            authorize(&grants, Operation::Search, &many, Some(level(0))),
            Err(Denial::TooManyNamespaces)
        );
        // A read-only grant never yields write authority.
        let scope = authorize(&grants, Operation::Search, &["a".into()], Some(level(9))).unwrap();
        assert_eq!(scope.write_max, None);
    }

    #[test]
    fn memory_authority_operation_classification_is_total_and_writes_need_write_authority() {
        use Operation as O;
        // Every operation is a read, a write or both; the classification is pinned here so a
        // change to it is a visible decision (D3, P2 notes).
        for op in Operation::ALL {
            assert!(
                op.reads() || op.writes(),
                "{op:?} is neither a read nor a write"
            );
        }
        let both: Vec<_> = Operation::ALL
            .into_iter()
            .filter(|op| op.reads() && op.writes())
            .collect();
        assert_eq!(both, vec![O::Correct, O::Delete]);
        assert!(O::Purge.writes() && !O::Purge.reads());
        assert!(O::GenerationStatus.reads() && !O::GenerationStatus.writes());
        // A grant row without write_max (from any source) is refused for writes, never treated
        // as level 0.
        let grants = vec![grant("a", Some(6), None, &[O::Save, O::Delete, O::Purge])];
        let name = ["a".to_string()];
        assert_eq!(
            authorize(&grants, O::Save, &name, None),
            Err(Denial::WriteNotGranted)
        );
        assert_eq!(
            authorize(&grants, O::Delete, &name, Some(level(6))),
            Err(Denial::WriteNotGranted)
        );
        assert_eq!(
            authorize(&grants, O::Purge, &name, None),
            Err(Denial::WriteNotGranted)
        );
        // Delete reads: it must name a ceiling within read_max.
        let grants = vec![grant("a", Some(3), Some(3), &[O::Delete])];
        assert_eq!(
            authorize(&grants, O::Delete, &name, None),
            Err(Denial::MissingReadCeiling)
        );
        assert_eq!(
            authorize(&grants, O::Delete, &name, Some(level(4))),
            Err(Denial::CeilingExceedsGrant)
        );
        assert!(authorize(&grants, O::Delete, &name, Some(level(3))).is_ok());
    }

    #[test]
    fn memory_authority_operation_names_round_trip() {
        for op in Operation::ALL {
            assert_eq!(Operation::parse(op.as_str()), Some(op));
        }
        assert_eq!(Operation::parse("admin"), None);
    }
}
