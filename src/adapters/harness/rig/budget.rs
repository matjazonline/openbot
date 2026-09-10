//! Conservative token reservation: one UTF-8 byte reserves one token, including JSON framing.
//! Unlike chars/4 this never understates byte-fallback text. Provider usage is still diagnostic.
use crate::app_error::{AppError, AppResult};
use std::sync::atomic::{AtomicUsize, Ordering};

pub const MAX_RUN_TOKEN_BUDGET: usize = 1_048_576;

pub struct RunBudget {
    remaining: AtomicUsize,
}

impl RunBudget {
    pub fn new(tokens: usize) -> AppResult<Self> {
        if tokens == 0 || tokens > MAX_RUN_TOKEN_BUDGET {
            return Err(AppError::BadRequest("Invalid Rig run token budget".into()));
        }
        Ok(Self {
            remaining: AtomicUsize::new(tokens),
        })
    }

    pub fn charge(&self, tokens: usize) -> AppResult<()> {
        self.remaining
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                remaining.checked_sub(tokens)
            })
            .map(|_| ())
            .map_err(|_| AppError::BadRequest("Rig run token budget exhausted".into()))
    }

    pub fn remaining(&self) -> usize {
        self.remaining.load(Ordering::SeqCst)
    }
}
