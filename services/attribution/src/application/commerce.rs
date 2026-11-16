//! Immutable merchant facts, bounded integer money, and reconciliation outcomes.
use super::{
    catalog::{AdId, ClickId},
    identity::{Failure, Result},
    ids::entity_id,
    setup::StoreId,
    website::{AttributionReason, CheckoutId, timestamp},
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

entity_id!(OrderId);
entity_id!(EventId);
entity_id!(RefundId);
pub const MAX_AMOUNT: i64 = 9_000_000_000_000;
pub const MAX_ATTEMPTS: i32 = 20;
pub const PENDING_DAYS: i64 = 7;

macro_rules! external_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(pub String);
        impl TryFrom<String> for $name {
            type Error = &'static str;
            fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
                if value.is_empty()
                    || value.chars().count() > 128
                    || value.chars().any(|c| c.is_whitespace() || c.is_control())
                {
                    return Err("invalid external identifier");
                }
                Ok(Self(value))
            }
        }
        impl From<$name> for String {
            fn from(id: $name) -> String {
                id.0
            }
        }
    };
}
external_id!(EventKey);
external_id!(OrderKey);
external_id!(RefundKey);
external_id!(LineId);
external_id!(ProductId);

#[derive(Clone, Copy)]
pub struct CommercePolicy {
    pub event_max_age_days: u32,
    pub refund_window_days: u32,
}
impl CommercePolicy {
    pub fn new(event_max_age_days: u32, refund_window_days: u32) -> Result<Self> {
        if !(1..=3650).contains(&event_max_age_days) || !(1..=3650).contains(&refund_window_days) {
            return Err(Failure::Invalid);
        }
        Ok(Self {
            event_max_age_days,
            refund_window_days,
        })
    }
    pub fn delivery(&self, time: DateTime<Utc>, now: DateTime<Utc>) -> Result<()> {
        if time < now - Duration::days(i64::from(self.event_max_age_days)) {
            Err(Failure::Gone)
        } else if time > now + Duration::seconds(30) {
            Err(Failure::Invalid)
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Currency {
    EUR,
    USD,
    GBP,
    JPY,
}
impl Currency {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EUR => "EUR",
            Self::USD => "USD",
            Self::GBP => "GBP",
            Self::JPY => "JPY",
        }
    }
    pub fn exponent(self) -> u8 {
        if self == Self::JPY { 0 } else { 2 }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrderLine {
    pub line_id: LineId,
    pub product_id: ProductId,
    pub quantity: u32,
    pub merchandise_amount: i64,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefundLine {
    pub line_id: LineId,
    pub returned_quantity: u32,
    pub merchandise_amount: i64,
}
fn required_option<'de, T: Deserialize<'de>, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<T>, D::Error> {
    Option::deserialize(d)
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaidEvent {
    pub event_id: EventKey,
    pub order_id: OrderKey,
    #[serde(deserialize_with = "timestamp")]
    pub occurred_at: DateTime<Utc>,
    #[serde(deserialize_with = "timestamp")]
    pub order_created_at: DateTime<Utc>,
    #[serde(deserialize_with = "timestamp")]
    pub paid_at: DateTime<Utc>,
    #[serde(deserialize_with = "required_option")]
    pub checkout_id: Option<CheckoutId>,
    pub currency: Currency,
    pub lines: Vec<OrderLine>,
    pub merchandise_amount: i64,
    pub tax_amount: i64,
    pub shipping_amount: i64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefundEvent {
    pub event_id: EventKey,
    pub order_id: OrderKey,
    #[serde(deserialize_with = "timestamp")]
    pub occurred_at: DateTime<Utc>,
    pub refund_id: RefundKey,
    pub currency: Currency,
    pub lines: Vec<RefundLine>,
    pub tax_amount: i64,
    pub shipping_amount: i64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CancellationEvent {
    pub event_id: EventKey,
    pub order_id: OrderKey,
    #[serde(deserialize_with = "timestamp")]
    pub occurred_at: DateTime<Utc>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Conversion {
    Paid(PaidEvent),
    Refund(RefundEvent),
    Cancelled(CancellationEvent),
}
pub fn amount(value: i64) -> Result<i64> {
    if (0..=MAX_AMOUNT).contains(&value) {
        Ok(value)
    } else {
        Err(Failure::Invalid)
    }
}
pub fn total(values: impl IntoIterator<Item = i64>) -> Result<i64> {
    values.into_iter().try_fold(0i64, |sum, value| {
        amount(value)?;
        amount(sum.checked_add(value).ok_or(Failure::Invalid)?)
    })
}
impl Conversion {
    pub fn event_key(&self) -> &EventKey {
        match self {
            Self::Paid(e) => &e.event_id,
            Self::Refund(e) => &e.event_id,
            Self::Cancelled(e) => &e.event_id,
        }
    }
    pub fn order_key(&self) -> &OrderKey {
        match self {
            Self::Paid(e) => &e.order_id,
            Self::Refund(e) => &e.order_id,
            Self::Cancelled(e) => &e.order_id,
        }
    }
    pub fn occurred_at(&self) -> DateTime<Utc> {
        match self {
            Self::Paid(e) => e.occurred_at,
            Self::Refund(e) => e.occurred_at,
            Self::Cancelled(e) => e.occurred_at,
        }
    }
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Paid(_) => "paid",
            Self::Refund(_) => "refund",
            Self::Cancelled(_) => "cancelled",
        }
    }
    pub fn refund_key(&self) -> Option<&RefundKey> {
        match self {
            Self::Refund(e) => Some(&e.refund_id),
            _ => None,
        }
    }
    pub fn normalize(mut self) -> Result<Self> {
        match &mut self {
            Self::Paid(e) => {
                e.lines.sort_by(|a, b| a.line_id.cmp(&b.line_id));
                if e.lines.is_empty()
                    || e.lines.len() > 100
                    || e.lines.windows(2).any(|w| w[0].line_id == w[1].line_id)
                    || e.lines
                        .iter()
                        .any(|l| !(1..=1_000_000).contains(&l.quantity))
                    || e.order_created_at > e.paid_at
                    || e.paid_at > e.occurred_at
                {
                    return Err(Failure::Invalid);
                }
                if total(e.lines.iter().map(|l| l.merchandise_amount))?
                    != amount(e.merchandise_amount)?
                {
                    return Err(Failure::Invalid);
                }
                total([e.merchandise_amount, e.tax_amount, e.shipping_amount])?;
            }
            Self::Refund(e) => {
                e.lines.sort_by(|a, b| a.line_id.cmp(&b.line_id));
                if e.lines.is_empty()
                    || e.lines.len() > 100
                    || e.lines.windows(2).any(|w| w[0].line_id == w[1].line_id)
                    || e.lines.iter().any(|l| l.returned_quantity > 1_000_000)
                {
                    return Err(Failure::Invalid);
                }
                total(
                    e.lines
                        .iter()
                        .map(|l| l.merchandise_amount)
                        .chain([e.tax_amount, e.shipping_amount]),
                )?;
            }
            Self::Cancelled(_) => {}
        }
        Ok(self)
    }
    pub fn validate_delivery(&self, policy: CommercePolicy, now: DateTime<Utc>) -> Result<()> {
        policy.delivery(self.occurred_at(), now)?;
        if let Self::Paid(e) = self {
            policy.delivery(e.paid_at, now)?;
        }
        Ok(())
    }
    /// Delivery metadata is not the immutable payment/refund business identity.
    pub fn business_fact(&self) -> Result<Value> {
        let mut value = serde_json::to_value(self).map_err(|_| Failure::Unavailable)?;
        let fields = value.as_object_mut().ok_or(Failure::Unavailable)?;
        fields.remove("event_id");
        if matches!(self, Self::Paid(_)) {
            fields.remove("occurred_at");
        }
        Ok(value)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventState {
    Accepted,
    Pending,
    Conflict,
    Unresolved,
}
impl EventState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Pending => "pending",
            Self::Conflict => "conflict",
            Self::Unresolved => "unresolved",
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventReason {
    PaymentMissing,
    PaidConflict,
    CheckoutTimeMismatch,
    RefundConflict,
    CurrencyMismatch,
    UnknownLine,
    RefundExceedsBalance,
    RefundBeforePayment,
    RefundWindowExpired,
    RetryExhausted,
    DeadlineExpired,
    TemporarilyUnavailable,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct EventResult {
    pub id: EventId,
    pub event_id: EventKey,
    pub state: EventState,
    pub reason: Option<EventReason>,
    pub order_id: Option<OrderId>,
    pub kind: String,
    pub external_order_id: OrderKey,
    pub refund_id: Option<RefundKey>,
    pub occurred_at: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
    pub attempts: i32,
    pub next_attempt_at: Option<DateTime<Utc>>,
    pub deadline_at: DateTime<Utc>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: OrderId,
    pub store_id: StoreId,
    pub external_order_id: OrderKey,
    pub order_created_at: DateTime<Utc>,
    pub paid_at: DateTime<Utc>,
    pub currency: Currency,
    pub lines: Vec<OrderLine>,
    pub merchandise_amount: i64,
    pub tax_amount: i64,
    pub shipping_amount: i64,
    pub refunded_merchandise_amount: i64,
    pub refunded_tax_amount: i64,
    pub refunded_shipping_amount: i64,
    pub returned_units: i64,
    pub click_id: Option<ClickId>,
    pub ad_id: Option<AdId>,
    pub attribution_reason: AttributionReason,
    pub policy: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Refund {
    pub refund_id: RefundKey,
    pub occurred_at: DateTime<Utc>,
    pub currency: Currency,
    pub lines: Vec<RefundLine>,
    pub tax_amount: i64,
    pub shipping_amount: i64,
}
pub struct LineBalance {
    pub line_id: LineId,
    pub quantity: i64,
    pub merchandise_amount: i64,
    pub returned_quantity: i64,
    pub refunded_amount: i64,
}
pub struct PaymentBalance {
    pub paid_at: DateTime<Utc>,
    pub currency: Currency,
    pub tax_remaining: i64,
    pub shipping_remaining: i64,
    pub lines: Vec<LineBalance>,
}
pub fn refund_conflict(
    refund: &RefundEvent,
    paid: &PaymentBalance,
    policy: CommercePolicy,
) -> Option<EventReason> {
    if refund.currency != paid.currency {
        return Some(EventReason::CurrencyMismatch);
    }
    if refund.occurred_at < paid.paid_at {
        return Some(EventReason::RefundBeforePayment);
    }
    if refund.occurred_at > paid.paid_at + Duration::days(i64::from(policy.refund_window_days)) {
        return Some(EventReason::RefundWindowExpired);
    }
    if refund.tax_amount > paid.tax_remaining || refund.shipping_amount > paid.shipping_remaining {
        return Some(EventReason::RefundExceedsBalance);
    }
    for line in &refund.lines {
        let Some(balance) = paid.lines.iter().find(|b| b.line_id == line.line_id) else {
            return Some(EventReason::UnknownLine);
        };
        if i64::from(line.returned_quantity) > balance.quantity - balance.returned_quantity
            || line.merchandise_amount > balance.merchandise_amount - balance.refunded_amount
        {
            return Some(EventReason::RefundExceedsBalance);
        }
    }
    None
}
