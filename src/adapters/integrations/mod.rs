//! Provider-specific integration adapters, distinct from login OAuth.
pub(crate) mod google_credentials;
pub(crate) mod owner;

pub(crate) mod google_authorization;
pub(crate) mod google_http;
pub(crate) mod oauth_state;

pub(crate) mod google_renewal;
