//! Connected-account contracts. No host, persistence, or transport dependencies.
pub mod account;
pub mod authority;
pub mod ids;
pub use account::*;
pub use ids::*;

pub const MAX_CREDENTIAL_BYTES: usize = 16 * 1024;
pub const MAX_METADATA_BYTES: usize = 4096;
pub const MAX_SCOPES: usize = 64;
pub const MAX_ACCOUNTS: usize = 128;
pub const MAX_GRANTS: usize = 128;
pub const MAX_PAGE_SIZE: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IntegrationError {
    #[error("Invalid integration value")]
    Invalid,
    #[error("Integration resource not found")]
    NotFound,
    #[error("Integration authority denied")]
    Denied,
    #[error("Integration revision conflict")]
    Conflict,
    #[error("Integration limit reached")]
    Limit,
    #[error("Integration method is unavailable")]
    Unavailable,
    #[error("Integration storage unavailable")]
    Storage,
    #[error("Integration credential unavailable")]
    Credential,
}
pub type IntegrationResult<T> = Result<T, IntegrationError>;
