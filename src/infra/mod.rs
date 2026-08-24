use crate::{
    adapters::{
        crypto::argon2::ArgonPasswordHasher,
        persistence::{PostgresPersistence, credentials::CredentialCipher},
    },
    infra::db::init_db,
};

pub mod app;
pub mod config;
pub mod db;
pub mod events;
pub mod runtime_metrics;
pub mod setup;

pub async fn postgres_persistence() -> anyhow::Result<PostgresPersistence> {
    let pool = init_db().await?;
    let persistence =
        PostgresPersistence::with_credential_cipher(pool, CredentialCipher::from_env()?);
    persistence.rotate_credentials().await?;
    Ok(persistence)
}

pub fn argon2_password_hasher() -> ArgonPasswordHasher {
    ArgonPasswordHasher::default()
}
