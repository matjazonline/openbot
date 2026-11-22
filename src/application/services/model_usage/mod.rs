//! Company-manager reporting. Prices estimate usage; they never authorize model work.
use crate::{
    app_error::{AppError, AppResult},
    entities::value_objects::{ModelName, ModelProvider},
};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenPrice(i64);
impl TokenPrice {
    /// USD per million tokens, with at most six decimal places. No exponent or rounding.
    pub fn parse(value: &str) -> AppResult<Self> {
        let invalid = || {
            AppError::BadRequest(
                "Price must be 0–1000000 USD with at most six decimal places".into(),
            )
        };
        if value.is_empty() || value.len() > 14 {
            return Err(invalid());
        }
        let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
        if whole.is_empty()
            || !whole.bytes().all(|c| c.is_ascii_digit())
            || fraction.len() > 6
            || !fraction.bytes().all(|c| c.is_ascii_digit())
            || (value.contains('.') && fraction.is_empty())
        {
            return Err(invalid());
        }
        let fraction_digits = fraction.len() as u32;
        let whole: i64 = whole.parse().map_err(|_| invalid())?;
        let fraction: i64 = if fraction.is_empty() {
            0
        } else {
            fraction.parse().map_err(|_| invalid())?
        };
        let micros = whole
            .checked_mul(1_000_000)
            .and_then(|v| v.checked_add(fraction * 10_i64.pow(6 - fraction_digits)))
            .ok_or_else(invalid)?;
        Self::from_micros(micros)
    }
    pub fn from_micros(value: i64) -> AppResult<Self> {
        if !(0..=1_000_000_000_000).contains(&value) {
            return Err(AppError::BadRequest("Price exceeds supported range".into()));
        }
        Ok(Self(value))
    }
    pub fn micros(self) -> i64 {
        self.0
    }
    pub fn decimal(self) -> String {
        format!("{}.{:06}", self.0 / 1_000_000, self.0 % 1_000_000)
    }
}

// JSON uses exact decimal USD per million, not an unlabeled fixed-point integer.
impl Serialize for TokenPrice {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.decimal())
    }
}

#[derive(Clone, Copy)]
pub struct UsageScope {
    pub company: Uuid,
    pub actor: Uuid,
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UsageGrouping {
    Model,
    Workflow,
    /// By the department the call is attributed to: its run's write unit when it was made
    /// (Plan07 02C), so archived usage keeps it too.
    Department,
    /// By the run's desk, within one department (`UsageQuery::unit`): the drill-down.
    Desk,
}
#[derive(Clone)]
pub struct UsageQuery {
    pub scope: UsageScope,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub grouping: UsageGrouping,
    pub agent: Option<Uuid>,
    pub workflow: Option<Uuid>,
    /// Only calls attributed to this department (Plan07 02C drill-down).
    pub unit: Option<Uuid>,
    /// Whether the per-department spend rows include archived departments (Plan08 02C.3).
    pub archived: crate::entities::department::ArchivedUnits,
}
impl UsageQuery {
    pub fn validate(&self) -> AppResult<()> {
        if self.start >= self.end || self.end - self.start > Duration::days(90) {
            return Err(AppError::BadRequest(
                "Usage range must be positive and at most 90 days".into(),
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct ModelPrice {
    pub provider: ModelProvider,
    pub model: ModelName,
    pub input: TokenPrice,
    pub output: TokenPrice,
    pub revision: i64,
}
#[derive(Clone, Copy)]
pub struct PriceRates {
    pub input: TokenPrice,
    pub output: TokenPrice,
}
#[derive(Clone)]
pub struct PriceWrite {
    pub scope: UsageScope,
    pub provider: ModelProvider,
    pub model: ModelName,
    pub expected_revision: Option<i64>,
    /// None removes a configured rate; unpriced models remain usable.
    pub rates: Option<PriceRates>,
}
impl PriceWrite {
    pub fn validate(&self) -> AppResult<()> {
        if self.provider.is_empty()
            || self.provider.len() > 128
            || self.provider != ModelProvider::canonical(&self.provider)
            || self.model.is_empty()
            || self.model.len() > 256
            || self.model != ModelName::canonical(&self.model)
            || self.expected_revision.is_some_and(|r| r <= 0)
            || (self.rates.is_none() && self.expected_revision.is_none())
        {
            return Err(AppError::BadRequest(
                "Invalid model price identity or revision".into(),
            ));
        }
        Ok(())
    }
}
/// The department a usage group is for; `id` is `None` for "No department".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UsageDepartment {
    pub id: Option<Uuid>,
    pub name: String,
}

/// The desk a usage group is for; `id` is `None` for calls with no live desk (archived usage, or
/// runs without one).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UsageDesk {
    pub id: Option<Uuid>,
    pub name: String,
}

/// One department's spend counted toward a cap for the current UTC day or month (Plan07 02C):
/// what admission enforces, from the running totals, apart from the ledger figures above it.
#[derive(Debug, Clone, Serialize)]
pub struct UnitBudgetStatus {
    pub unit: Uuid,
    pub name: String,
    /// The department is archived (Plan08 02C.3): its caps and spend are kept and shown on ask.
    pub archived: bool,
    /// `day` or `month`.
    pub period: String,
    /// Exact USD decimals.
    pub cap_usd: Option<String>,
    pub spent_usd: String,
    pub remaining_usd: Option<String>,
    /// Tokens of models with no configured price, which count zero toward caps (decision h).
    pub unpriced_tokens: u64,
}

/// Micro-USD as an exact USD decimal.
pub fn usd(micros: i64) -> String {
    let sign = if micros < 0 { "-" } else { "" };
    let micros = micros.unsigned_abs();
    format!("{sign}{}.{:06}", micros / 1_000_000, micros % 1_000_000)
}

#[derive(Debug, Serialize)]
pub struct UsageGroup {
    pub provider: Option<ModelProvider>,
    pub model: Option<ModelName>,
    pub workflow: Option<Uuid>,
    /// Set when grouping by department: the department, or `None` for runs on a desk no
    /// department owns, runs without a desk, and archived usage, which keeps no run.
    pub department: Option<UsageDepartment>,
    /// Set when grouping by desk.
    pub desk: Option<UsageDesk>,
    pub calls: u64,
    pub completed_calls: u64,
    pub unconfirmed_calls: u64,
    pub reported_calls: u64,
    pub estimated_calls: u64,
    pub mixed_calls: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Exact USD decimal at current configured rates; excludes unpriced calls.
    pub priced_subtotal_usd: Option<String>,
    pub unpriced_calls: u64,
}
#[derive(Debug, Serialize)]
pub struct UsageReport {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub grouping: UsageGrouping,
    pub groups: Vec<UsageGroup>,
    pub scanned_calls: usize,
    pub truncated: bool,
    pub prices: Vec<ModelPrice>,
    pub prices_truncated: bool,
    /// Caps and counted spend of the departments the viewer manages.
    pub budgets: Vec<UnitBudgetStatus>,
    pub budgets_truncated: bool,
    /// Prices are the company's; only root managers change them (Plan07 G1 r2 D9).
    pub prices_editable: bool,
}
#[async_trait]
pub trait ModelUsageStore: Send + Sync {
    /// Rechecks current manager membership inside the same transaction as the read/write.
    async fn usage_report(&self, query: UsageQuery) -> AppResult<UsageReport>;
    async fn save_model_price(&self, write: PriceWrite) -> AppResult<()>;
    /// The viewer's departments' caps and counted spend for the current UTC day and month (the
    /// dashboard panel), and whether the list was cut; not-found for someone managing nothing.
    async fn unit_spend(&self, scope: UsageScope) -> AppResult<(Vec<UnitBudgetStatus>, bool)>;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prices_are_exact_bounded_decimals() {
        for (text, micros) in [
            ("0", 0),
            ("1.000001", 1_000_001),
            ("0.000001", 1),
            ("1000000", 1_000_000_000_000),
        ] {
            let price = TokenPrice::parse(text).unwrap();
            assert_eq!(price.micros(), micros);
            assert_eq!(serde_json::to_value(price).unwrap(), price.decimal());
            assert_eq!(TokenPrice::parse(&price.decimal()).unwrap(), price);
        }
        for text in [
            "",
            "-1",
            "NaN",
            "1e2",
            ".1",
            "1.",
            "1.0000001",
            "1000000.000001",
            " 1",
            "1.2.3",
        ] {
            assert!(TokenPrice::parse(text).is_err(), "{text}");
        }
    }
}

mod aggregate;
pub use aggregate::{UsageSample, aggregate};
