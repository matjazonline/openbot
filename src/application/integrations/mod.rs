//! Application-owned gateway. Credential-bearing ports are crate-private.
pub mod execution;
pub mod gateway;
pub(crate) mod private;
pub(crate) mod registry;
pub use execution::*;
pub use gateway::*;

#[cfg(test)]
mod tests;
pub use registry::{AppDefinition, AuthConfig, AuthProvider};
pub(crate) mod lifecycle;
pub mod management;
pub(crate) mod unavailable;

impl From<crate::domain::integrations::IntegrationError> for crate::app_error::AppError {
    fn from(error: crate::domain::integrations::IntegrationError) -> Self {
        use crate::domain::integrations::IntegrationError as E;
        match error {
            E::NotFound => Self::NotFound("Vault account not found".into()),
            E::Denied => Self::Forbidden("Vault access denied".into()),
            E::Conflict => {
                Self::Conflict("Vault account changed; reload before trying again".into())
            }
            E::Invalid => Self::BadRequest("Invalid vault input".into()),
            E::Limit => Self::BadRequest("Vault limit reached".into()),
            E::Unavailable => Self::ServiceUnavailable("Integration feature unavailable".into()),
            E::Credential => Self::ServiceUnavailable("Vault credential unavailable".into()),
            E::Storage => Self::ServiceUnavailable("Vault storage unavailable".into()),
        }
    }
}

pub(crate) mod grants;

pub(crate) mod connect;

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "Private credential resolution is consumed by authenticated business transports in phase06"
    )
)]
pub(crate) mod renewal;
