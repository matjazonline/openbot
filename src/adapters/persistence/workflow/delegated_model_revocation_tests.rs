use super::*;

#[tokio::test]
async fn delegated_admin_competing_agent_grant_removal_fences_initial_preparation() {
    for capability in ["view", "participate"] {
        let f = Arc::new(ModelAuthority::new(Provenance::OwnedReply).await);
        f.remove_owner_view().await;
        let mut revoke = f.revocation(capability).await;
        let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *revoke)
            .await
            .unwrap();
        let caller = tokio::spawn({
            let f = f.clone();
            async move {
                f.p()
                    .prepare_agent(&f.claim, LeasePolicy::new(Duration::from_secs(60)).unwrap())
                    .await
            }
        });
        blocked_by(f.p().pool(), blocker).await;
        assert!(!caller.is_finished());
        revoke.commit().await.unwrap();
        assert!(matches!(caller.await.unwrap(), Err(AppError::NotFound(_))));
    }
}

#[tokio::test]
async fn delegated_admin_competing_agent_grant_removal_fences_current_model_source() {
    for capability in ["view", "participate"] {
        let f = Arc::new(ModelAuthority::new(Provenance::OwnedReply).await);
        f.remove_owner_view().await;
        let prepared = f.prepared().await;
        let mut revoke = f.revocation(capability).await;
        let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *revoke)
            .await
            .unwrap();
        let caller = tokio::spawn({
            let f = f.clone();
            async move { f.current_transaction(&prepared).await.map(|_| ()) }
        });
        blocked_by(f.p().pool(), blocker).await;
        assert!(!caller.is_finished());
        revoke.commit().await.unwrap();
        assert!(matches!(caller.await.unwrap(), Err(AppError::NotFound(_))));
    }
}

#[tokio::test]
async fn delegated_admin_competing_current_model_authority_fences_grant_removal() {
    for capability in ["view", "participate"] {
        let f = Arc::new(ModelAuthority::new(Provenance::OwnedReply).await);
        f.remove_owner_view().await;
        let prepared = f.prepared().await;
        let mut authority = f.current_transaction(&prepared).await.unwrap();
        let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *authority)
            .await
            .unwrap();
        let remover = tokio::spawn({
            let f = f.clone();
            async move {
                f.revocation(capability).await.commit().await.unwrap();
            }
        });
        blocked_by(f.p().pool(), blocker).await;
        assert!(!remover.is_finished());
        authority.commit().await.unwrap();
        remover.await.unwrap();
        assert!(matches!(
            f.current_transaction(&prepared).await,
            Err(AppError::NotFound(_))
        ));
    }
}

#[tokio::test]
async fn delegated_admin_model_source_never_borrows_owned_reply_proof_for_other_runs() {
    for provenance in [
        Provenance::Observer,
        Provenance::Manual,
        Provenance::Schedule,
    ] {
        let f = ModelAuthority::new(provenance).await;
        let prepared = f.prepared().await; // Original owner has the existing human View exception.
        f.remove_owner_view().await;
        let owned: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM channel_reply_runs WHERE company_id=$1 AND run_id=$2",
        )
        .bind(f.scope().company.as_uuid())
        .bind(f.scope().run.as_uuid())
        .fetch_one(f.p().pool())
        .await
        .unwrap();
        assert_eq!(owned, 0);
        assert!(matches!(
            f.current_transaction(&prepared).await,
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            f.p()
                .prepare_agent(&f.claim, LeasePolicy::new(Duration::from_secs(60)).unwrap())
                .await,
            Err(AppError::NotFound(_))
        ));
    }
}
