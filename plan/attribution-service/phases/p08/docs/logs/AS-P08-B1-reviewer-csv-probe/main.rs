pub use attribution_service::application;
#[path = "/private/var/folders/y1/v99y__1j741fh8z9fqd8rvlc0000gn/T/as-attribution-author-7fo_yda4/worktree/services/attribution/src/adapters/analytics/export.rs"]
mod export;
use application::analytics::*;
fn main() {
    let metrics=Metrics::from([("paid_orders".into(),1),("attributed_paid_orders".into(),1),("merchandise_amount".into(),100)]);
    let currency=|s:&str|FinancialTotals::new(serde_json::from_str(s).unwrap(),&metrics);
    let group=|key:&str|Group {
        group:Some(key.into()),raw_clicks:1,eligible_clicks:1,image_loads:0,landing_page_loads:0,visible_placements:0,
        attributed_paid_orders:2,unattributed_paid_orders:0,unattributed_reasons:REASONS.into_iter().map(|s|(s.into(),0)).collect(),
        period_conversion_rate:None,rate_denominator:"omitted_sold_product_filter",currencies:vec![currency("\"EUR\""),currency("\"USD\"")]
    };
    let report=Analytics {start:"2026-11-15T00:00:00Z".parse().unwrap(),end:"2026-11-16T00:00:00Z".parse().unwrap(),timezone:"UTC".into(),groups:vec![group("product-x"),group("product-y")],next_cursor:None};
    print!("{}",export::csv(&report).unwrap());
}
