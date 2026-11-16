//! Wall-clock timestamps are durable evidence, never the sole elapsed-age authority.
use super::identity::{CompanyId, CompanyStatus, Failure, LifecycleState, Result};
use chrono::{DateTime, Utc};
use std::{collections::HashMap, sync::Mutex, time::Duration};
use tokio::time::Instant;
const MAX_TICKETS: usize = 1024;
const MAX_AGE: Duration = Duration::from_secs(300);
pub struct Observation {
    pub number: i64,
    pub started_at: DateTime<Utc>,
}
#[derive(Clone)]
pub struct Ticket {
    company: CompanyId,
    version: i64,
    deadline: Instant,
}
impl Ticket {
    pub fn check(&self, company: CompanyId, version: i64) -> Result<()> {
        if self.company != company || self.version != version || Instant::now() >= self.deadline {
            return Err(Failure::Unavailable);
        }
        Ok(())
    }
}
#[derive(Default)]
pub struct Freshness {
    tickets: Mutex<HashMap<CompanyId, Ticket>>,
}
impl Freshness {
    pub fn invalidate(&self, company: CompanyId) {
        self.tickets.lock().unwrap().remove(&company);
    }

    pub fn ticket(&self, company: CompanyId) -> Option<Ticket> {
        self.tickets
            .lock()
            .unwrap()
            .get(&company)
            .filter(|ticket| Instant::now() < ticket.deadline)
            .cloned()
    }
    pub fn record(&self, status: &CompanyStatus, start: Instant) -> Result<Ticket> {
        let mut tickets = self.tickets.lock().unwrap();
        if status.state != LifecycleState::Active {
            tickets.remove(&status.company_id);
            return Err(Failure::Forbidden);
        }
        let ticket = Ticket {
            company: status.company_id,
            version: status.version,
            deadline: start + MAX_AGE,
        };
        ticket.check(status.company_id, status.version)?;
        if let Some(previous) = tickets.get(&status.company_id)
            && previous.version == ticket.version
            && previous.deadline > ticket.deadline
        {
            return Ok(previous.clone());
        }
        if tickets.len() >= MAX_TICKETS && !tickets.contains_key(&status.company_id) {
            let oldest = tickets
                .iter()
                .min_by_key(|(_, ticket)| ticket.deadline)
                .map(|(id, _)| *id);
            if let Some(oldest) = oldest {
                tickets.remove(&oldest);
            }
        }
        tickets.insert(status.company_id, ticket.clone());
        Ok(ticket)
    }
}
