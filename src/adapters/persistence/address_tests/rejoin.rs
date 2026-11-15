//! Restoring a personal target preserves history without reviving old transport authority.
use super::*;
use crate::{
    application::ports::thread::ThreadPersistence,
    entities::{access_level::AccessLevel, company_member::CompanyAccessRole, thread::Thread},
    use_cases::company_invite::CreateCompanyInvite,
    use_cases::user::{
        AccountChange, AccountChangeKind, AccountChangePersistence, PendingAccountChange,
        auth_attempt_key,
    },
};

struct RejoinCase {
    fixture: Fixture,
    user: Uuid,
    old_email: EmailAddress,
    email: EmailAddress,
    original: PrincipalId,
}

struct OutsiderPlaces {
    desk: Uuid,
    thread: Thread,
    outsider: PrincipalId,
}

impl RejoinCase {
    async fn new() -> Option<Self> {
        let fixture = Fixture::new().await?;
        let (user, _) = account(&fixture.persistence, "changed_rejoin").await;
        let old_email = EmailAddress::from(email_of(fixture.pool(), user).await);
        sqlx::query("UPDATE users SET email_verified_at = CURRENT_TIMESTAMP WHERE id = $1")
            .bind(user)
            .execute(fixture.pool())
            .await
            .unwrap();
        accept_invite(&fixture, user, &old_email).await;
        let original = PrincipalId::new(fixture.principal_of(user).await);
        fixture
            .persistence
            .remove_member(fixture.company.id, user)
            .await
            .unwrap();
        let email = EmailAddress::from(format!("changed-{}@example.test", Uuid::new_v4().simple()));
        change_email(&fixture, user, &email).await;
        Some(Self {
            fixture,
            user,
            old_email,
            email,
            original,
        })
    }

    async fn restore(&self) {
        accept_invite(&self.fixture, self.user, &self.email).await;
        assert_eq!(
            self.fixture.principal_of(self.user).await,
            self.original.as_uuid()
        );
        assert_eq!(
            self.fixture
                .addresses_of(self.original.as_uuid())
                .await
                .len(),
            1
        );
    }

    async fn outsider_places(&self) -> OutsiderPlaces {
        let desk = ChannelPersistence::create(
            &self.fixture.persistence,
            self.fixture.company.id,
            ChannelWrite {
                name: "Current email grant".into(),
                slug: "current-email".into(),
                participant_emails: Some(vec![self.email.to_string()]),
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        let thread = ThreadPersistence::create_thread(
            &self.fixture.persistence,
            self.fixture.company.id,
            crate::entities::address::AddressTarget::Desk(desk.id),
            "Existing correspondence",
            std::slice::from_ref(&self.email),
        )
        .await
        .unwrap();
        let outsider = identity_context(&self.fixture, &self.email)
            .await
            .principal_id
            .unwrap();
        assert_ne!(outsider, self.original);
        OutsiderPlaces {
            desk: desk.id,
            thread,
            outsider,
        }
    }
}

pub(super) async fn change_email(fixture: &Fixture, user: Uuid, email: &EmailAddress) {
    fixture
        .persistence
        .save_pending_account_change(PendingAccountChange {
            user_id: user,
            change: AccountChange::Email(email),
            confirmation_code_hash: "confirmed-rejoin-change",
            expires_at: chrono::Utc::now() + chrono::Duration::minutes(15),
        })
        .await
        .unwrap();
    let attempt = auth_attempt_key(
        "rejoin-regression",
        "account-change-code:v1",
        &[user.as_bytes()],
    );
    let changed = fixture
        .persistence
        .confirm_pending_account_change(
            user,
            AccountChangeKind::Email,
            "confirmed-rejoin-change",
            &attempt,
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(changed.email, email.as_str());
    assert!(changed.email_verified_at.is_some());
}

async fn accept_invite(f: &Fixture, user: Uuid, email: &EmailAddress) {
    let invite = f
        .persistence
        .create_invite(CreateCompanyInvite {
            actor_id: f.owner,
            company_id: f.company.id,
            email: email.clone(),
            role: CompanyAccessRole::Member,
            access_level: AccessLevel::try_from(1_i16).unwrap(),
        })
        .await
        .unwrap();
    assert!(
        f.persistence
            .accept_pending_invite(invite.id, user, email)
            .await
            .unwrap()
            .is_some()
    );
}

async fn identity_context(f: &Fixture, email: &EmailAddress) -> PrincipalAccessContext {
    let identity = EmailIdentity::parse(email.clone())
        .unwrap()
        .qualify_default();
    f.persistence
        .access_context_for_identity(f.company.id, &identity)
        .await
        .unwrap()
}

#[tokio::test]
async fn direct_thread_changed_email_rejoin_does_not_revive_old_email_authority() {
    let Some(case) = RejoinCase::new().await else {
        return;
    };
    assert!(
        !identity_context(&case.fixture, &case.old_email)
            .await
            .membership
            .is_team()
    );
    case.restore().await;
    let obsolete = identity_context(&case.fixture, &case.old_email).await;
    assert_eq!(obsolete.membership, CompanyMembership::None);
    assert_ne!(obsolete.principal_id, Some(case.original));
    let current = identity_context(&case.fixture, &case.email).await;
    assert_eq!(current.membership, CompanyMembership::Member);
    assert_eq!(current.principal_id, Some(case.original));

    // A new account can verify the former mailbox, but cannot acquire the old personal target.
    let name = format!("replacement_{}", Uuid::new_v4().simple());
    case.fixture
        .persistence
        .create_user(&name, &case.old_email, "hash")
        .await
        .unwrap();
    let replacement = case
        .fixture
        .persistence
        .get_by_email(&case.old_email)
        .await
        .unwrap()
        .unwrap();
    sqlx::query("UPDATE users SET email_verified_at = CURRENT_TIMESTAMP WHERE id=$1")
        .bind(replacement.id)
        .execute(case.fixture.pool())
        .await
        .unwrap();
    accept_invite(&case.fixture, replacement.id, &case.old_email).await;
    assert_ne!(
        case.fixture.principal_of(replacement.id).await,
        case.original.as_uuid()
    );
    assert_eq!(
        case.fixture.principal_of(case.user).await,
        case.original.as_uuid()
    );
}

#[tokio::test]
async fn direct_thread_changed_email_rejoin_adopts_current_outsider_grants_and_thread_places() {
    let Some(case) = RejoinCase::new().await else {
        return;
    };
    let OutsiderPlaces {
        desk,
        thread,
        outsider,
    } = case.outsider_places().await;
    case.restore().await;
    let viewer = case
        .fixture
        .persistence
        .access_context_for_user(case.fixture.company.id, case.user)
        .await
        .unwrap()
        .unwrap();
    let desk = ChannelPersistence::get_by_id(&case.fixture.persistence, desk)
        .await
        .unwrap()
        .unwrap();
    assert!(desk.viewer_access(viewer));
    assert_eq!(desk.principal_grants.len(), 2);
    assert!(
        desk.principal_grants
            .iter()
            .all(|grant| grant.principal_id == case.original)
    );
    let thread = case
        .fixture
        .persistence
        .get_thread_by_id(case.fixture.company.id, thread.id)
        .await
        .unwrap()
        .unwrap();
    assert!(thread.contains_principal(case.original));
    assert!(!thread.contains_principal(outsider));
    assert_eq!(
        identity_context(&case.fixture, &case.email)
            .await
            .principal_id,
        Some(case.original)
    );
}

#[tokio::test]
async fn direct_thread_changed_email_rejoin_keeps_places_on_a_still_reachable_outsider() {
    let Some(case) = RejoinCase::new().await else {
        return;
    };
    let OutsiderPlaces {
        desk,
        thread,
        outsider,
    } = case.outsider_places().await;
    let alias = EmailAddress::from(format!("alias-{}@example.test", Uuid::new_v4().simple()));
    sqlx::query(
        "INSERT INTO participant_identities(id, company_id, principal_id, transport, namespace, subject, status, claim_metadata, provenance) \
         SELECT $1, company_id, principal_id, transport, namespace, $4, 'observed', claim_metadata, 'transport_ingress' \
         FROM participant_identities WHERE company_id=$2 AND principal_id=$3 LIMIT 1",
    ).bind(Uuid::new_v4()).bind(case.fixture.company.id).bind(outsider.as_uuid()).bind(alias.as_str())
        .execute(case.fixture.pool()).await.unwrap();
    case.restore().await;
    let desk = ChannelPersistence::get_by_id(&case.fixture.persistence, desk)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(desk.principal_grants.len(), 2);
    assert!(
        desk.principal_grants
            .iter()
            .all(|grant| grant.principal_id == outsider)
    );
    let thread = case
        .fixture
        .persistence
        .get_thread_by_id(case.fixture.company.id, thread.id)
        .await
        .unwrap()
        .unwrap();
    assert!(thread.contains_principal(outsider));
    assert!(!thread.contains_principal(case.original));
    assert_eq!(
        identity_context(&case.fixture, &alias).await.principal_id,
        Some(outsider)
    );
    assert_eq!(
        identity_context(&case.fixture, &case.email)
            .await
            .principal_id,
        Some(case.original)
    );
}
