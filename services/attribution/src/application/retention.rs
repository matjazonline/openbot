//! Retention cannot erase evidence within an admitted delivery/refund horizon.
use super::{
    commerce::CommercePolicy,
    identity::{Failure, Result},
};
#[derive(Clone, Copy)]
pub struct RetentionPolicy {
    pub clicks: u32,
    pub observations: u32,
    pub financial: u32,
    pub events: u32,
    pub audit: u32,
}
impl RetentionPolicy {
    pub fn new(
        commerce: CommercePolicy,
        clicks: u32,
        observations: u32,
        financial: u32,
        events: u32,
        audit: u32,
    ) -> Result<Self> {
        if [clicks, observations, financial, events, audit]
            .iter()
            .any(|v| !(1..=10000).contains(v))
            || clicks < 31 + commerce.event_max_age_days
            || observations < 2
            || financial < commerce.refund_window_days + commerce.event_max_age_days + 1
            || events < commerce.event_max_age_days + 1
        {
            return Err(Failure::Invalid);
        }
        Ok(Self {
            clicks,
            observations,
            financial,
            events,
            audit,
        })
    }
}
