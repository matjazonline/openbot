//! Reporting meanings and bounds, independent of transport and storage.
use super::{
    catalog::{AdId, CampaignId},
    commerce::{Currency, MAX_AMOUNT, ProductId},
    identity::{Failure, Result},
    setup::StoreId,
    website::parse_time,
};
use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_PAGE: u16 = 100;
pub const MAX_CURSOR: usize = 1024;
pub const MAX_EXPORT_ROWS: usize = 10_000;
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Grouping {
    Day,
    Ad,
    Campaign,
    AdvertisedProduct,
    SoldProduct,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub start: String,
    pub end: String,
    pub timezone: String,
    pub store_id: Option<StoreId>,
    pub ad_id: Option<AdId>,
    pub campaign_id: Option<CampaignId>,
    pub advertised_product_id: Option<ProductId>,
    pub sold_product_id: Option<ProductId>,
    pub currency: Option<Currency>,
    pub group_by: Option<Grouping>,
    pub limit: Option<u16>,
    pub cursor: Option<String>,
}
#[derive(Clone, Serialize)]
pub struct Filter {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub timezone: String,
    pub store_id: Option<StoreId>,
    pub ad_id: Option<AdId>,
    pub campaign_id: Option<CampaignId>,
    pub advertised_product_id: Option<ProductId>,
    pub sold_product_id: Option<ProductId>,
    pub currency: Option<Currency>,
    pub group_by: Option<Grouping>,
    #[serde(skip)]
    pub limit: u16,
    #[serde(skip)]
    pub cursor: Option<String>,
}
impl Query {
    pub fn validate(self) -> Result<Filter> {
        let start = parse_time(&self.start)?;
        let end = parse_time(&self.end)?;
        if start >= end || end - start > Duration::days(93) || self.timezone.len() > 64 {
            return Err(Failure::Invalid);
        }
        let zone = self.timezone.parse::<Tz>().map_err(|_| Failure::Invalid)?;
        let limit = self.limit.unwrap_or(25);
        if !(1..=MAX_PAGE).contains(&limit)
            || self
                .cursor
                .as_ref()
                .is_some_and(|c| c.is_empty() || c.len() > MAX_CURSOR)
        {
            return Err(Failure::Invalid);
        }
        Ok(Filter {
            start,
            end,
            timezone: zone.name().into(),
            store_id: self.store_id,
            ad_id: self.ad_id,
            campaign_id: self.campaign_id,
            advertised_product_id: self.advertised_product_id,
            sold_product_id: self.sold_product_id,
            currency: self.currency,
            group_by: self.group_by,
            limit,
            cursor: self.cursor,
        })
    }
}
impl Filter {
    pub fn omit_rate(&self) -> bool {
        self.sold_product_id.is_some() || self.group_by == Some(Grouping::SoldProduct)
    }
}
/// Local dates in the dashboard become explicit instants; a skipped midnight is
/// rejected and an ambiguous midnight uses the first occurrence.
pub fn midnight(date: &str, zone: &str) -> Result<DateTime<Utc>> {
    if date.len() != 10 || zone.len() > 64 {
        return Err(Failure::Invalid);
    }
    let date = NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| Failure::Invalid)?;
    let zone = zone.parse::<Tz>().map_err(|_| Failure::Invalid)?;
    zone.from_local_datetime(&date.and_hms_opt(0, 0, 0).ok_or(Failure::Invalid)?)
        .earliest()
        .map(|t| t.with_timezone(&Utc))
        .ok_or(Failure::Invalid)
}
pub fn bounded(value: &str) -> Result<i64> {
    let value = value.parse::<i64>().map_err(|_| Failure::Invalid)?;
    if (0..=MAX_AMOUNT).contains(&value) {
        Ok(value)
    } else {
        Err(Failure::Invalid)
    }
}
pub fn add(a: i64, b: i64) -> Result<i64> {
    a.checked_add(b)
        .filter(|n| (0..=MAX_AMOUNT).contains(n))
        .ok_or(Failure::Invalid)
}
#[derive(Clone, Serialize)]
pub struct FinancialTotals {
    pub currency: Currency,
    pub paid_orders: i64,
    pub attributed_paid_orders: i64,
    pub sold_units: i64,
    pub returned_units: i64,
    pub merchandise_amount: i64,
    pub refunded_merchandise_amount: i64,
    pub net_merchandise_amount: i64,
    pub tax_amount: i64,
    pub shipping_amount: i64,
    pub refunded_tax_amount: i64,
    pub refunded_shipping_amount: i64,
    pub refunds: i64,
}
pub type Metrics = BTreeMap<String, i64>;
pub fn metric(metrics: &Metrics, key: &str) -> i64 {
    metrics.get(key).copied().unwrap_or(0)
}
impl FinancialTotals {
    pub fn new(currency: Currency, m: &Metrics) -> Self {
        Self {
            currency,
            paid_orders: metric(m, "paid_orders"),
            attributed_paid_orders: metric(m, "attributed_paid_orders"),
            sold_units: metric(m, "sold_units"),
            returned_units: metric(m, "returned_units"),
            merchandise_amount: metric(m, "merchandise_amount"),
            refunded_merchandise_amount: metric(m, "refunded_merchandise_amount"),
            net_merchandise_amount: metric(m, "merchandise_amount")
                - metric(m, "refunded_merchandise_amount"),
            tax_amount: metric(m, "tax_amount"),
            shipping_amount: metric(m, "shipping_amount"),
            refunded_tax_amount: metric(m, "refunded_tax_amount"),
            refunded_shipping_amount: metric(m, "refunded_shipping_amount"),
            refunds: metric(m, "refunds"),
        }
    }
}
pub const REASONS: [&str; 7] = [
    "missing",
    "no_consent",
    "expired",
    "future",
    "bot",
    "foreign_or_missing",
    "evidence_unavailable",
];
#[derive(Clone, Serialize)]
pub struct Group {
    pub group: Option<String>,
    pub raw_clicks: i64,
    pub eligible_clicks: i64,
    pub image_loads: i64,
    pub landing_page_loads: i64,
    pub visible_placements: i64,
    pub attributed_paid_orders: i64,
    pub unattributed_paid_orders: i64,
    pub unattributed_reasons: BTreeMap<String, i64>,
    pub period_conversion_rate: Option<f64>,
    pub rate_denominator: &'static str,
    pub currencies: Vec<FinancialTotals>,
}
impl Group {
    pub fn build(
        key: Option<String>,
        metrics: BTreeMap<Option<String>, Metrics>,
        filter: &Filter,
    ) -> Result<Self> {
        let empty = Metrics::new();
        let clicks = metrics.get(&None).unwrap_or(&empty);
        let mut currencies = Vec::new();
        let mut attributed = 0;
        let mut unattributed = 0;
        let mut reasons: BTreeMap<String, i64> =
            REASONS.into_iter().map(|r| (r.into(), 0)).collect();
        for (currency, m) in &metrics {
            let Some(currency) = currency else {
                continue;
            };
            let currency = serde_json::from_value(serde_json::Value::String(currency.clone()))
                .map_err(|_| Failure::Unavailable)?;
            let totals = FinancialTotals::new(currency, m);
            attributed = add(attributed, totals.attributed_paid_orders)?;
            for reason in REASONS {
                let amount = metric(m, &format!("reason_{reason}"));
                unattributed = add(unattributed, amount)?;
                let previous = reasons.get_mut(reason).ok_or(Failure::Unavailable)?;
                *previous = add(*previous, amount)?;
            }
            currencies.push(totals);
        }
        let eligible = metric(clicks, "eligible_clicks");
        Ok(Self {
            group: key,
            raw_clicks: metric(clicks, "raw_clicks"),
            eligible_clicks: eligible,
            image_loads: metric(clicks, "image_loads"),
            landing_page_loads: metric(clicks, "landing_page_loads"),
            visible_placements: metric(clicks, "visible_placements"),
            attributed_paid_orders: attributed,
            unattributed_paid_orders: unattributed,
            unattributed_reasons: reasons,
            period_conversion_rate: if eligible == 0 || filter.omit_rate() {
                None
            } else {
                Some(attributed as f64 / eligible as f64)
            },
            rate_denominator: if filter.omit_rate() {
                "omitted_sold_product_filter"
            } else {
                "eligible_clicks_in_period"
            },
            currencies,
        })
    }
}
#[derive(Serialize)]
pub struct Analytics {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub timezone: String,
    pub groups: Vec<Group>,
    pub next_cursor: Option<String>,
}
