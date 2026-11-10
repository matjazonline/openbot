//! The ordered 0–9 access-level contract (plan 01 "Access level contract", decisions D4).
//! Pure: no I/O. Every save label in the service is computed here.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A classification in the ordered range 0 (public) through 9.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "i64", into = "i64")]
pub struct AccessLevel(u8);

impl AccessLevel {
    pub const PUBLIC: Self = Self(0);
    pub const MAX: Self = Self(9);

    pub fn new(value: i64) -> Result<Self, LevelOutOfRange> {
        u8::try_from(value)
            .ok()
            .filter(|value| *value <= 9)
            .map(Self)
            .ok_or(LevelOutOfRange(value))
    }
    pub const fn value(self) -> u8 {
        self.0
    }
    pub const fn as_i16(self) -> i16 {
        self.0 as i16
    }
    pub fn all() -> impl Iterator<Item = Self> {
        (0..=9).map(Self)
    }
    /// A record at `self` is visible under `ceiling`.
    pub const fn visible_under(self, ceiling: Self) -> bool {
        self.0 <= ceiling.0
    }
}

impl TryFrom<i64> for AccessLevel {
    type Error = LevelOutOfRange;
    fn try_from(value: i64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<AccessLevel> for i64 {
    fn from(level: AccessLevel) -> Self {
        i64::from(level.0)
    }
}
impl JsonSchema for AccessLevel {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "AccessLevel".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "integer",
            "minimum": 0,
            "maximum": 9,
            "description": "Ordered classification; 0 is public."
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("access level {0} is outside 0..=9")]
pub struct LevelOutOfRange(pub i64);

/// The highest level of everything returned by a read; 0 when nothing was returned.
pub fn exposed_level(levels: impl IntoIterator<Item = AccessLevel>) -> AccessLevel {
    levels.into_iter().max().unwrap_or(AccessLevel::PUBLIC)
}

/// Consumption evidence for the context that produced a memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Consumed {
    /// Precise accounting: the highest level actually exposed to the producing context.
    Exact { level: AccessLevel },
    /// Accounting incomplete: the producing context's ceiling is the watermark.
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WatermarkKind {
    Exact,
    CeilingFallback,
}

/// The host-supplied facts a save label is derived from. All of them enter the canonical
/// idempotency request, so the label is a pure function of hashed fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelInputs {
    /// The producing context's trusted authorized ceiling; `None` means it is unavailable.
    pub producing_ceiling: Option<AccessLevel>,
    pub consumed: Consumed,
    /// Levels of service-side sources (immutable revisions or parent observations).
    pub service_sources: Vec<AccessLevel>,
    /// Client-attested levels of external contributing sources.
    pub external_sources: Vec<AccessLevel>,
    /// Optional consistency check supplied by the host.
    pub expected: Option<AccessLevel>,
}

/// A derived, persisted label. The constructor is the only way to obtain one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SaveLabel {
    level: AccessLevel,
    producing_ceiling: AccessLevel,
    watermark_kind: WatermarkKind,
    watermark_level: AccessLevel,
    source_max_level: AccessLevel,
}

impl SaveLabel {
    pub const fn level(self) -> AccessLevel {
        self.level
    }
    pub const fn producing_ceiling(self) -> AccessLevel {
        self.producing_ceiling
    }
    pub const fn watermark_kind(self) -> WatermarkKind {
        self.watermark_kind
    }
    pub const fn watermark_level(self) -> AccessLevel {
        self.watermark_level
    }
    pub const fn source_max_level(self) -> AccessLevel {
        self.source_max_level
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LabelRefusal {
    #[error("the producing context's trusted ceiling is unavailable")]
    MissingTrustedCeiling,
    #[error("consumed level {consumed:?} exceeds the producing ceiling {ceiling:?}")]
    ConsumedExceedsCeiling {
        consumed: AccessLevel,
        ceiling: AccessLevel,
    },
    #[error("a known source at {source_level:?} exceeds the producing ceiling {ceiling:?}")]
    SourceAboveCeiling {
        source_level: AccessLevel,
        ceiling: AccessLevel,
    },
    #[error("expected level {expected:?} differs from the derived level {derived:?}")]
    InconsistentClassification {
        expected: AccessLevel,
        derived: AccessLevel,
    },
    #[error("derived level {level:?} exceeds the write grant")]
    AboveWriteGrant { level: AccessLevel },
}

impl LabelRefusal {
    pub const fn reason(self) -> &'static str {
        match self {
            Self::MissingTrustedCeiling => "missing_trusted_ceiling",
            Self::ConsumedExceedsCeiling { .. } => "consumed_exceeds_ceiling",
            Self::SourceAboveCeiling { .. } => "source_above_ceiling",
            Self::InconsistentClassification { .. } => "inconsistent_classification",
            Self::AboveWriteGrant { .. } => "above_write_grant",
        }
    }
}

/// `save_level = max(watermark, known contributing source levels)`, where the watermark is the
/// proven consumed level or, without precise evidence, the producing context's ceiling.
pub fn derive_label(
    inputs: &LabelInputs,
    write_max: Option<AccessLevel>,
) -> Result<SaveLabel, LabelRefusal> {
    let ceiling = inputs
        .producing_ceiling
        .ok_or(LabelRefusal::MissingTrustedCeiling)?;
    let (watermark_kind, watermark_level) = match inputs.consumed {
        Consumed::Exact { level } if level > ceiling => {
            return Err(LabelRefusal::ConsumedExceedsCeiling {
                consumed: level,
                ceiling,
            });
        }
        Consumed::Exact { level } => (WatermarkKind::Exact, level),
        Consumed::Unknown => (WatermarkKind::CeilingFallback, ceiling),
    };
    let source_max_level = exposed_level(
        inputs
            .service_sources
            .iter()
            .chain(&inputs.external_sources)
            .copied(),
    );
    if source_max_level > ceiling {
        return Err(LabelRefusal::SourceAboveCeiling {
            source_level: source_max_level,
            ceiling,
        });
    }
    let level = watermark_level.max(source_max_level);
    if let Some(expected) = inputs.expected
        && expected != level
    {
        return Err(LabelRefusal::InconsistentClassification {
            expected,
            derived: level,
        });
    }
    if write_max.is_none_or(|write_max| level > write_max) {
        return Err(LabelRefusal::AboveWriteGrant { level });
    }
    Ok(SaveLabel {
        level,
        producing_ceiling: ceiling,
        watermark_kind,
        watermark_level,
        source_max_level,
    })
}

/// A correction never lowers an existing record's classification.
pub fn corrected_level(existing: AccessLevel, recomputed: SaveLabel) -> AccessLevel {
    existing.max(recomputed.level())
}

#[cfg(test)]
#[path = "access_level_tests.rs"]
mod tests;
