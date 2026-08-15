use sqlx::PgPool;

use crate::app_error::AppError;

pub mod agent;
pub mod approval;
pub mod channel;
pub mod company;
pub mod company_invite;
pub mod task;
pub mod thread;
pub mod user;

#[derive(Clone)]
pub struct PostgresPersistence {
    pool: PgPool,
}

impl PostgresPersistence {
    pub fn new(pool: PgPool) -> Self {
        PostgresPersistence { pool }
    }
}

impl From<sqlx::Error> for AppError {
    fn from(value: sqlx::Error) -> Self {
        AppError::Database(value.to_string())
    }
}
