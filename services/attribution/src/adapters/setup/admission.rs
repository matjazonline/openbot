use crate::application::identity::{CompanyId, Failure, Result};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

/// Entries live exactly as long as admitted requests; untrusted IDs cannot grow it.
#[derive(Clone, Default)]
pub struct Admission(Arc<Mutex<HashMap<CompanyId, usize>>>);
pub struct Permit {
    admission: Admission,
    company: CompanyId,
}
impl Admission {
    pub fn enter(&self, company: CompanyId) -> Result<Permit> {
        let mut counts = self.0.lock().map_err(|_| Failure::Unavailable)?;
        if counts.len() >= 1024 && !counts.contains_key(&company) {
            return Err(Failure::RateLimited);
        }
        let count = counts.entry(company).or_default();
        if *count >= 8 {
            return Err(Failure::RateLimited);
        }
        *count += 1;
        Ok(Permit {
            admission: self.clone(),
            company,
        })
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        if let Ok(mut counts) = self.admission.0.lock()
            && let Some(n) = counts.get_mut(&self.company)
        {
            *n -= 1;
            if *n == 0 {
                counts.remove(&self.company);
            }
        }
    }
}
