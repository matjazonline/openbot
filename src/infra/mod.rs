use crate::{
    adapters::{
        crypto::argon2::ArgonPasswordHasher,
        persistence::{PostgresPersistence, credentials::CredentialCipher},
    },
    infra::db::init_db,
};
use tracing::info;

pub mod app;
pub mod config;
pub mod db;
pub mod events;
pub mod runtime_metrics;
pub mod setup;

pub async fn postgres_persistence() -> anyhow::Result<PostgresPersistence> {
    let pool = init_db().await?;
    let credential_cipher = CredentialCipher::from_env()?;
    info!(
        active_version = credential_cipher.active_version(),
        available_versions = ?credential_cipher.available_versions(),
        "Credential encryption configuration validated"
    );
    Ok(PostgresPersistence::with_credential_cipher(
        pool,
        credential_cipher,
    ))
}

pub fn argon2_password_hasher() -> ArgonPasswordHasher {
    ArgonPasswordHasher::default()
}
