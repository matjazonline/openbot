use super::{test_support::*, *};
use crate::adapters::persistence::test_support::{
    own_database, revoke_membership, wait_until_a_backend_is_blocked,
};
use crate::use_cases::user::auth_attempt_key;
use std::sync::Arc;
use tokio::sync::Barrier;

#[tokio::test]
async fn cloudroom_pairing_competing_claimants_and_lost_response_recover_only_winner() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let issued = f
        .cases
        .create_pairing(f.owner, f.company, None)
        .await
        .unwrap();
    let first = Candidate::new(issued.code.clone());
    let second = Candidate::new(issued.code);
    let barrier = Arc::new(Barrier::new(2));
    let a = async {
        barrier.wait().await;
        f.persistence.claim_pairing(&first.claim).await.unwrap()
    };
    let b = async {
        barrier.wait().await;
        f.persistence.claim_pairing(&second.claim).await.unwrap()
    };
    let (a, b) = tokio::join!(a, b);
    assert_ne!(a.is_some(), b.is_some());
    let (winner, loser) = if a.is_some() {
        (&first, &second)
    } else {
        (&second, &first)
    };
    let machine = a.or(b).unwrap().machine;
    // Discarding the HTTP result does not require a new secret or a second enrollment.
    let replay = f
        .persistence
        .claim_pairing(&winner.claim)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(replay.machine.id, machine.id);
    let status = f
        .cases
        .pair_status(
            "127.0.0.1".parse().unwrap(),
            winner.claim.installation_id,
            winner.secret.clone(),
        )
        .await
        .unwrap();
    assert_eq!(status.machine.id, machine.id);
    assert!(
        f.persistence
            .claim_pairing(&loser.claim)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        f.persistence
            .pairing_status(winner.claim.installation_id, loser.secret.verifier())
            .await
            .unwrap()
            .is_none()
    );
    let mut changed = winner.claim.clone();
    changed.credential_verifier = MachineCredential::generate().unwrap().verifier();
    assert!(
        f.persistence
            .claim_pairing(&changed)
            .await
            .unwrap()
            .is_none()
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM local_machines")
        .fetch_one(&f.database.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    let stored: Vec<u8> =
        sqlx::query_scalar("SELECT verifier FROM local_machine_credentials WHERE machine_id=$1")
            .bind(machine.id.as_uuid())
            .fetch_one(&f.database.pool)
            .await
            .unwrap();
    assert_eq!(stored, winner.credential.verifier().as_bytes());
    assert_ne!(
        stored.as_slice(),
        winner.credential.expose_secret().as_bytes()
    );
}

#[tokio::test]
async fn cloudroom_pairing_expiry_uses_database_time_after_waiting_for_locks() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let issued = f
        .cases
        .create_pairing(f.owner, f.company, None)
        .await
        .unwrap();
    let candidate = Candidate::new(issued.code);
    let mut blocker = f.database.pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(f.company)
        .execute(&mut *blocker)
        .await
        .unwrap();
    let claim = f.persistence.claim_pairing(&candidate.claim);
    let expire = async {
        wait_until_a_backend_is_blocked(&f.database.pool).await;
        // Claim's transaction has started. A transaction-start timestamp would now be stale.
        sqlx::query("UPDATE local_pairings SET expires_at=clock_timestamp()+INTERVAL '50 milliseconds' WHERE id=$1")
            .bind(issued.attempt.id.as_uuid()).execute(&mut *blocker).await.unwrap();
        sqlx::query("SELECT pg_sleep(0.1)")
            .execute(&mut *blocker)
            .await
            .unwrap();
        blocker.commit().await.unwrap();
    };
    let (result, ()) = tokio::join!(claim, expire);
    assert!(result.unwrap().is_none());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM local_machines")
        .fetch_one(&f.database.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn cloudroom_company_live_issuer_revocation_and_management_scope() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    assert!(matches!(
        f.cases.create_pairing(f.member, f.company, None).await,
        Err(AppError::Forbidden(_))
    ));
    assert!(matches!(
        f.cases.create_pairing(f.stranger, f.company, None).await,
        Err(AppError::NotFound(_))
    ));
    let issued = f
        .cases
        .create_pairing(f.admin, f.company, None)
        .await
        .unwrap();
    let candidate = Candidate::new(issued.code);
    let mut blocker = f.database.pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(f.company)
        .execute(&mut *blocker)
        .await
        .unwrap();
    let claim = f.persistence.claim_pairing(&candidate.claim);
    let withdraw = async {
        wait_until_a_backend_is_blocked(&f.database.pool).await;
        revoke_membership(&mut blocker, f.company, f.admin).await;
        blocker.commit().await.unwrap();
    };
    let (result, ()) = tokio::join!(claim, withdraw);
    assert!(result.unwrap().is_none());
    let paired = f.pair(f.company).await;
    assert!(matches!(
        f.cases
            .machine(f.owner, f.foreign, paired.receipt.machine.id)
            .await,
        Err(AppError::NotFound(_))
    ));
    assert!(matches!(
        f.cases
            .change(
                f.owner,
                f.foreign,
                paired.receipt.machine.id,
                MachineChange::Revoke
            )
            .await,
        Err(AppError::NotFound(_))
    ));
    let dead = sqlx::postgres::PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_millis(100))
        .connect_lazy("postgres://nobody@127.0.0.1:9/unavailable")
        .unwrap();
    let dead = Arc::new(PostgresPersistence::new(dead));
    let cases = LocalMachineUseCases::new(
        dead.clone(),
        dead,
        "independent-unavailable-db-test-key".into(),
    );
    assert!(matches!(
        cases.machines(f.owner, f.company, None).await,
        Err(AppError::Database(_) | AppError::DatabaseTimeout(_))
    ));
}

#[tokio::test]
async fn cloudroom_pairing_code_limit_cancel_and_competing_attempt_budget() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let mut issued = Vec::new();
    for _ in 0..MAX_PAIRING_CODES {
        issued.push(
            f.cases
                .create_pairing(f.owner, f.company, None)
                .await
                .unwrap(),
        );
    }
    assert!(matches!(
        f.cases.create_pairing(f.owner, f.company, None).await,
        Err(AppError::Conflict(_))
    ));
    assert!(matches!(
        f.cases
            .cancel_pairing(f.owner, f.foreign, issued[0].attempt.id)
            .await,
        Err(AppError::NotFound(_))
    ));
    f.cases
        .cancel_pairing(f.owner, f.company, issued[0].attempt.id)
        .await
        .unwrap();
    f.cases
        .create_pairing(f.owner, f.company, None)
        .await
        .unwrap();
    let key = auth_attempt_key("test-budget-key", "cloudroom-test", &[b"shared"]);
    for _ in 0..11 {
        assert!(f.persistence.reserve_pairing_budget(&[key]).await.unwrap());
    }
    let barrier = Barrier::new(2);
    let a = async {
        barrier.wait().await;
        f.persistence.reserve_pairing_budget(&[key]).await.unwrap()
    };
    let b = async {
        barrier.wait().await;
        f.persistence.reserve_pairing_budget(&[key]).await.unwrap()
    };
    let (a, b) = tokio::join!(a, b);
    assert_ne!(a, b);
    let attempts: i16 =
        sqlx::query_scalar("SELECT attempts FROM local_pair_budgets WHERE identity_key=$1")
            .bind(key.as_bytes().as_slice())
            .fetch_one(&f.database.pool)
            .await
            .unwrap();
    assert_eq!(attempts, 12);
    assert!(!f.persistence.reserve_pairing_budget(&[key]).await.unwrap());
}

#[tokio::test]
async fn cloudroom_pairing_budget_cardinality_is_bounded_under_competing_new_keys() {
    let Some(database) = own_database().await else {
        return;
    };
    let p = PostgresPersistence::new(database.pool.clone());
    sqlx::query("INSERT INTO local_pair_budgets (identity_key,attempts,window_ends_at) \
        SELECT decode(md5(value::text)||md5(('second-'||value)::text),'hex'),1,clock_timestamp()+INTERVAL '2 minutes' FROM generate_series(1,4095) AS value")
        .execute(&database.pool).await.unwrap();
    let a = auth_attempt_key("test-budget-key", "cloudroom-test", &[b"a"]);
    let b = auth_attempt_key("test-budget-key", "cloudroom-test", &[b"b"]);
    let barrier = Barrier::new(2);
    let first = async {
        barrier.wait().await;
        p.reserve_pairing_budget(&[a]).await.unwrap()
    };
    let second = async {
        barrier.wait().await;
        p.reserve_pairing_budget(&[b]).await.unwrap()
    };
    let (first, second) = tokio::join!(first, second);
    assert_ne!(first, second);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM local_pair_budgets")
        .fetch_one(&database.pool)
        .await
        .unwrap();
    assert_eq!(count, 4096);
    sqlx::query(
        "UPDATE local_pair_budgets SET window_ends_at=clock_timestamp()-INTERVAL '1 second'",
    )
    .execute(&database.pool)
    .await
    .unwrap();
    assert!(p.reserve_pairing_budget(&[a]).await.unwrap());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM local_pair_budgets")
        .fetch_one(&database.pool)
        .await
        .unwrap();
    assert!((3996..=3997).contains(&count)); // One bounded cleanup batch, plus the new reservation.
}

#[test]
fn cloudroom_pairing_secret_encodings_entropy_and_debug_redaction() {
    let code = PairingCode::generate().unwrap();
    assert_eq!(code.expose_secret().len(), 32);
    let credential = MachineCredential::generate().unwrap();
    assert_eq!(credential.expose_secret().len(), 64);
    assert!(!format!("{code:?} {credential:?}").contains(code.expose_secret()));
    assert!(!format!("{credential:?}").contains(credential.expose_secret()));
    for invalid in [
        "a".repeat(31),
        "A".repeat(32),
        "é".repeat(16),
        "a".repeat(33),
    ] {
        assert!(PairingCode::try_from(invalid).is_err());
    }
    assert!(MachineCredential::try_from("a".repeat(63)).is_err());
    assert!(serde_json::from_value::<CredentialVerifier>(json!("g".repeat(64))).is_err());
    assert!(validate_name(&"é".repeat(64)).is_ok());
    assert!(validate_name(&"é".repeat(65)).is_err());
}

#[tokio::test]
async fn cloudroom_pairing_reused_claim_or_credential_never_reserves_another_installation() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let existing = f.pair(f.company).await;
    let issued = f
        .cases
        .create_pairing(f.owner, f.foreign, None)
        .await
        .unwrap();
    let mut claim = Candidate::new(issued.code.clone());
    claim.claim.claim_verifier = existing.candidate.claim.claim_verifier;
    assert!(
        f.persistence
            .claim_pairing(&claim.claim)
            .await
            .unwrap()
            .is_none()
    );
    let mut credential = Candidate::new(issued.code.clone());
    credential.claim.credential_verifier = existing.candidate.claim.credential_verifier;
    assert!(
        f.persistence
            .claim_pairing(&credential.claim)
            .await
            .unwrap()
            .is_none()
    );
    let reservations: i64 = sqlx::query_scalar("SELECT count(*) FROM local_installations")
        .fetch_one(&f.database.pool)
        .await
        .unwrap();
    assert_eq!(reservations, 1);
    let fresh = Candidate::new(issued.code);
    assert!(
        f.persistence
            .claim_pairing(&fresh.claim)
            .await
            .unwrap()
            .is_some()
    );
    let machines: i64 = sqlx::query_scalar("SELECT count(*) FROM local_machines")
        .fetch_one(&f.database.pool)
        .await
        .unwrap();
    assert_eq!(machines, 2);
}

#[tokio::test]
async fn cloudroom_pairing_cancel_and_consume_are_linearized() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let issued = f
        .cases
        .create_pairing(f.owner, f.company, None)
        .await
        .unwrap();
    let candidate = Candidate::new(issued.code);
    let barrier = Barrier::new(2);
    let consume = async {
        barrier.wait().await;
        f.persistence.claim_pairing(&candidate.claim).await.unwrap()
    };
    let cancel = async {
        barrier.wait().await;
        f.cases
            .cancel_pairing(f.owner, f.company, issued.attempt.id)
            .await
    };
    let (consumed, cancelled) = tokio::join!(consume, cancel);
    let expected_machines = if cancelled.is_err() { 1 } else { 0 };
    match consumed {
        Some(_) => assert!(matches!(cancelled, Err(AppError::NotFound(_)))),
        None => cancelled.unwrap(),
    }
    let machines: i64 = sqlx::query_scalar("SELECT count(*) FROM local_machines")
        .fetch_one(&f.database.pool)
        .await
        .unwrap();
    assert_eq!(machines, expected_machines);
}
