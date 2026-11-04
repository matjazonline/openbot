//! Immutable run payloads. Conflict reuse must own the tuple before a run's deferred FK exists.
use super::*;
use sha2::{Digest, Sha256};

pub(super) struct BundleDigest([u8; 32]);
impl BundleDigest {
    pub(super) fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

pub(super) async fn insert_on(
    db: &mut PgConnection,
    company: CompanyId,
    bytes: &[u8],
) -> AppResult<BundleDigest> {
    let digest = BundleDigest(Sha256::digest(bytes).into());
    // WHERE false still locks the conflicting tuple, but performs no UPDATE and invokes no
    // immutable-payload trigger. GC either skips our lock, or wins first and we insert anew.
    sqlx::query(
        "INSERT INTO workflow_bundles(company_id,digest,bundle) VALUES ($1,$2,$3) \
         ON CONFLICT(company_id,digest) DO UPDATE SET bundle=EXCLUDED.bundle WHERE false",
    )
    .bind(company.as_uuid())
    .bind(digest.as_bytes())
    .bind(bytes)
    .execute(&mut *db)
    .await?;
    let stored: Vec<u8> =
        sqlx::query_scalar("SELECT bundle FROM workflow_bundles WHERE company_id=$1 AND digest=$2")
            .bind(company.as_uuid())
            .bind(digest.as_bytes())
            .fetch_one(db)
            .await?;
    if stored != bytes {
        return Err(AppError::Internal(
            "Stored workflow bundle does not match its content address".into(),
        ));
    }
    Ok(digest)
}
