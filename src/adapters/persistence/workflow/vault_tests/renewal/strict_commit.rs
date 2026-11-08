use super::*;
use crate::adapters::persistence::integrations::renewal_tests::commit_proxy::{CommitProxy, Fault};
#[tokio::test]
async fn integration_renewal_strict_lost_reservation_or_replacement_commit_ack_is_reread() {
    for fault in [Fault::PendingReservation, Fault::CredentialWrite] {
        let f = Box::pin(StrictSetup::new()).await;
        let (i, c) = prepared(&f.setup).await;
        let proxy = CommitProxy::new(f.setup.fixture.persistence.pool(), fault).await;
        let vault = super::super::commit::through(&f.vault, &proxy).await;
        let result = vault.resolve(&i, &c).await.unwrap();
        assert_eq!(result.generation.get(), 2);
        assert_eq!(proxy.dropped(), 1);
        assert_eq!(f.server.calls(), 1);
        assert!(!protocol::pending(&f.vault, &f.account).await);
        assert_eq!(
            vault.resolve(&i, &c).await.unwrap().generation,
            result.generation
        );
        assert_eq!(f.server.calls(), 1);
    }
}
