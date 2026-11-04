//! An address change re-keys the account's identity in every company, on both write paths.
//!
//! Everything asserted here is decided by SQL -- which principal an identity row points at, which
//! principal a message names as author, which rows the grants and thread places hang on -- so it
//! runs against a real database.

use std::sync::Arc;

use chrono::Utc;
use uuid::Uuid;

use crate::adapters::persistence::PostgresPersistence;
use crate::adapters::protocols::email::EmailIdentity;
use crate::app_error::AppError;
use crate::application::ports::channel::ChannelPersistence;
use crate::application::ports::channel::ChannelWrite;
use crate::application::ports::participant::IdentityObservation;
use crate::application::ports::participant::PrincipalAccessPersistence;
use crate::entities::company_member::CompanyAccessRole;
use crate::entities::company_member::CompanyMembership;
use crate::entities::participant::IdentityClaimMetadata;
use crate::entities::participant::IdentityProvenance;
use crate::entities::participant::PrincipalAccessContext;
use crate::entities::transport::QualifiedIdentity;
use crate::entities::user::User;
use crate::entities::value_objects::EmailAddress;
use crate::use_cases::company::CompanyPersistence;
use crate::use_cases::company::CompanyWrite;
use crate::use_cases::company_invite::CompanyInvitePersistence;
use crate::use_cases::participant::IdentityDirectory;
use crate::use_cases::user::AccountChange;
use crate::use_cases::user::AccountChangeKind;
use crate::use_cases::user::AccountChangePersistence;
use crate::use_cases::user::PendingAccountChange;
use crate::use_cases::user::ProfileUpdate;
use crate::use_cases::user::UserPersistence;
use crate::use_cases::user::auth_attempt_key;

use super::super::test_support::test_pool;

fn email_identity(address: &str) -> QualifiedIdentity {
    EmailIdentity::parse(EmailAddress::from(address))
        .unwrap()
        .qualify_default()
}

fn unique(label: &str) -> String {
    format!("{label}_{}", Uuid::new_v4().simple())
}

/// A verified account, so it may accept invites.
async fn verified_account(persistence: &PostgresPersistence, label: &str) -> User {
    let username = unique(label);
    let email = format!("{username}@example.test");
    let user = persistence
        .create_user(&username, &email, "hash")
        .await
        .unwrap();
    sqlx::query("UPDATE users SET email_verified_at = CURRENT_TIMESTAMP WHERE id = $1")
        .bind(user.id)
        .execute(persistence.pool())
        .await
        .unwrap();
    UserPersistence::get_by_id(persistence, user.id)
        .await
        .unwrap()
        .unwrap()
}

async fn company_of(persistence: &PostgresPersistence, owner: &User, label: &str) -> Uuid {
    CompanyPersistence::create(
        persistence,
        owner.id,
        CompanyWrite {
            name: format!("{label} Corp"),
            slug: unique(label).replace('_', "-"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap()
    .id
}

async fn join(persistence: &PostgresPersistence, company_id: Uuid, user: &User) {
    let invite = persistence
        .create_invite(company_id, &user.email, CompanyAccessRole::Member)
        .await
        .unwrap();
    persistence
        .accept_pending_invite(invite.id, user.id, &user.email)
        .await
        .unwrap()
        .expect("the invite is accepted");
}

async fn access(
    persistence: &PostgresPersistence,
    company_id: Uuid,
    address: &str,
) -> PrincipalAccessContext {
    persistence
        .access_context_for_identity(company_id, &email_identity(address))
        .await
        .unwrap()
}

async fn principal_kind(persistence: &PostgresPersistence, principal_id: Uuid) -> String {
    sqlx::query_scalar("SELECT kind FROM principals WHERE id = $1")
        .bind(principal_id)
        .fetch_one(persistence.pool())
        .await
        .unwrap()
}

async fn person_principal(
    persistence: &PostgresPersistence,
    company_id: Uuid,
    user: &User,
) -> Uuid {
    sqlx::query_scalar("SELECT id FROM principals WHERE company_id = $1 AND user_id = $2")
        .bind(company_id)
        .bind(user.id)
        .fetch_one(persistence.pool())
        .await
        .unwrap()
}

async fn identity_id(persistence: &PostgresPersistence, company_id: Uuid, address: &str) -> Uuid {
    sqlx::query_scalar(
        "SELECT id FROM participant_identities WHERE company_id = $1 AND subject = $2",
    )
    .bind(company_id)
    .bind(address.to_lowercase())
    .fetch_one(persistence.pool())
    .await
    .unwrap()
}

/// A message sent from `address` (resolved as mail from it would be) to `recipient`, written as
/// rows so the test owns exactly what history exists.
async fn mail_from(
    persistence: &PostgresPersistence,
    company_id: Uuid,
    address: &str,
    recipient: &str,
) -> Uuid {
    let sender = persistence
        .resolve_or_create_external_identity(
            company_id,
            IdentityObservation {
                identity: email_identity(address),
                display_label: None,
                claim_metadata: IdentityClaimMetadata::observation(),
                provenance: IdentityProvenance::TransportIngress,
            },
        )
        .await
        .unwrap();
    let to = persistence
        .resolve_or_create_external_identity(
            company_id,
            IdentityObservation {
                identity: email_identity(recipient),
                display_label: None,
                claim_metadata: IdentityClaimMetadata::observation(),
                provenance: IdentityProvenance::TransportIngress,
            },
        )
        .await
        .unwrap();
    let message_id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO messages
               (id, company_id, author_principal_id, authored_identity_id, subject,
                clean_text_body, direction, role, correlation_id, content_hash)
           VALUES ($1, $2, $3, $4, 'Hello', 'Hello', 'inbound', 'human', $5,
                   decode(repeat('ab', 32), 'hex'))"#,
    )
    .bind(message_id)
    .bind(company_id)
    .bind(sender.principal.id.as_uuid())
    .bind(sender.identity.id.as_uuid())
    .bind(Uuid::new_v4())
    .execute(persistence.pool())
    .await
    .unwrap();
    for (kind, identity) in [("sender", sender.identity.id), ("to", to.identity.id)] {
        sqlx::query(
            r#"INSERT INTO message_participants
                   (company_id, message_id, participant_identity_id, kind, position)
               VALUES ($1, $2, $3, $4, 0)"#,
        )
        .bind(company_id)
        .bind(message_id)
        .bind(identity.as_uuid())
        .bind(kind)
        .execute(persistence.pool())
        .await
        .unwrap();
    }
    message_id
}

async fn message_author(
    persistence: &PostgresPersistence,
    message_id: Uuid,
) -> (Uuid, Option<Uuid>) {
    sqlx::query_as("SELECT author_principal_id, authored_identity_id FROM messages WHERE id = $1")
        .bind(message_id)
        .fetch_one(persistence.pool())
        .await
        .unwrap()
}

async fn participant_identities(persistence: &PostgresPersistence, message_id: Uuid) -> Vec<Uuid> {
    sqlx::query_scalar(
        "SELECT participant_identity_id FROM message_participants WHERE message_id = $1 ORDER BY kind",
    )
    .bind(message_id)
    .fetch_all(persistence.pool())
    .await
    .unwrap()
}

/// The two ways an address changes.
#[derive(Clone, Copy)]
enum AddressChange {
    /// The code mailed to the new address comes back.
    Confirmed,
    /// The deployment installs addresses unverified.
    Bypass,
}

async fn change_address(
    persistence: &PostgresPersistence,
    user: &User,
    new_email: &str,
    path: AddressChange,
) -> User {
    let new_email = EmailAddress::from(new_email);
    match path {
        AddressChange::Confirmed => {
            persistence
                .save_pending_account_change(PendingAccountChange {
                    user_id: user.id,
                    change: AccountChange::Email(&new_email),
                    confirmation_code_hash: "code-hash",
                    expires_at: Utc::now() + chrono::Duration::minutes(15),
                })
                .await
                .unwrap();
            let key = auth_attempt_key(
                "test-attempt-secret",
                "account-change-code:v1",
                &[
                    user.id.as_bytes(),
                    AccountChangeKind::Email.as_str().as_bytes(),
                ],
            );
            persistence
                .confirm_pending_account_change(
                    user.id,
                    AccountChangeKind::Email,
                    "code-hash",
                    &key,
                )
                .await
                .unwrap()
                .expect("the change is confirmed")
        }
        AddressChange::Bypass => persistence
            .update_profile(
                user.id,
                ProfileUpdate {
                    username: &user.username,
                    email: &new_email,
                    avatar_url: None,
                },
            )
            .await
            .unwrap()
            .expect("the profile is saved"),
    }
}

/// The plan's acceptance case, on either write path: an owner of one company and member of another
/// changes address. Afterwards the old address is an outsider everywhere and the new one is the
/// person, and the mail the old address sent and received is still there.
async fn assert_address_change_moves_trust(path: AddressChange) {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let user = verified_account(&persistence, "mover").await;
    let other_owner = verified_account(&persistence, "host").await;
    let owned = company_of(&persistence, &user, "owned").await;
    let joined = company_of(&persistence, &other_owner, "joined").await;
    join(&persistence, joined, &user).await;
    let old = user.email.clone();
    let correspondent = format!("{}@partner.test", unique("partner"));

    // The old address both sent mail and received it, in each company.
    let mut history = Vec::new();
    for company_id in [owned, joined] {
        let sent = mail_from(&persistence, company_id, &old, &correspondent).await;
        let received = mail_from(&persistence, company_id, &correspondent, &old).await;
        history.push((company_id, sent, received));
    }
    let persons = [
        (
            owned,
            person_principal(&persistence, owned, &user).await,
            CompanyMembership::Owner,
        ),
        (
            joined,
            person_principal(&persistence, joined, &user).await,
            CompanyMembership::Member,
        ),
    ];
    let old_identities = [
        identity_id(&persistence, owned, &old).await,
        identity_id(&persistence, joined, &old).await,
    ];

    let new = format!("{}@example.test", unique("moved"));
    let changed = change_address(&persistence, &user, &new, path).await;
    assert_eq!(changed.email, new);

    for ((company_id, person, role), old_identity) in persons.into_iter().zip(old_identities) {
        let old_access = access(&persistence, company_id, &old).await;
        let outsider = old_access
            .principal_id
            .expect("the old address is still known");
        assert_ne!(
            outsider.as_uuid(),
            person,
            "the old address left the person"
        );
        assert_eq!(
            principal_kind(&persistence, outsider.as_uuid()).await,
            "external"
        );
        assert_eq!(old_access.membership, CompanyMembership::None);
        assert_eq!(
            identity_id(&persistence, company_id, &old).await,
            old_identity,
            "the identity row itself moved; it was not replaced"
        );
        let (status, provenance): (String, String) =
            sqlx::query_as("SELECT status, provenance FROM participant_identities WHERE id = $1")
                .bind(old_identity)
                .fetch_one(persistence.pool())
                .await
                .unwrap();
        assert_eq!(
            (status.as_str(), provenance.as_str()),
            ("observed", "transport_ingress")
        );

        let new_access = access(&persistence, company_id, &new).await;
        assert_eq!(new_access.principal_id.map(|id| id.as_uuid()), Some(person));
        assert_eq!(new_access.membership, role);
    }

    for (company_id, sent, received) in history {
        let outsider = access(&persistence, company_id, &old)
            .await
            .principal_id
            .unwrap()
            .as_uuid();
        let old_identity = identity_id(&persistence, company_id, &old).await;
        assert_eq!(
            message_author(&persistence, sent).await,
            (outsider, Some(old_identity)),
            "what the old address sent stays with the old address"
        );
        assert!(
            participant_identities(&persistence, sent)
                .await
                .contains(&old_identity),
            "its sender row survives"
        );
        assert!(
            participant_identities(&persistence, received)
                .await
                .contains(&old_identity),
            "the To row addressed to the old address survives"
        );
    }

    for company_id in [owned, joined] {
        let _ = CompanyPersistence::delete(&persistence, company_id).await;
    }
}

#[tokio::test]
async fn account_email_change_confirmed_moves_trust_in_every_company() {
    assert_address_change_moves_trust(AddressChange::Confirmed).await;
}

#[tokio::test]
async fn account_email_change_bypass_moves_trust_in_every_company() {
    assert_address_change_moves_trust(AddressChange::Bypass).await;
}

/// The detached address is an ordinary outsider: whoever registers it next and joins adopts it, as
/// the existing promotion adopts any outsider.
#[tokio::test]
async fn account_email_change_old_address_is_adopted_by_its_next_owner() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let user = verified_account(&persistence, "leaver").await;
    let company_id = company_of(&persistence, &user, "adopt").await;
    let old = user.email.clone();
    let new = format!("{}@example.test", unique("left"));
    change_address(&persistence, &user, &new, AddressChange::Confirmed).await;
    let outsider = access(&persistence, company_id, &old)
        .await
        .principal_id
        .unwrap();

    let successor = persistence
        .create_user(&unique("successor"), &old, "hash")
        .await
        .unwrap();
    sqlx::query("UPDATE users SET email_verified_at = CURRENT_TIMESTAMP WHERE id = $1")
        .bind(successor.id)
        .execute(persistence.pool())
        .await
        .unwrap();
    let successor = UserPersistence::get_by_id(&persistence, successor.id)
        .await
        .unwrap()
        .unwrap();
    join(&persistence, company_id, &successor).await;

    let adopted = access(&persistence, company_id, &old).await;
    assert_eq!(
        adopted.principal_id,
        Some(outsider),
        "the outsider is adopted, not duplicated"
    );
    assert_eq!(adopted.membership, CompanyMembership::Member);
    assert_eq!(
        principal_kind(&persistence, outsider.as_uuid()).await,
        "person"
    );
    // The first account still holds the company through its new address.
    assert_eq!(
        access(&persistence, company_id, &new).await.membership,
        CompanyMembership::Owner
    );

    let _ = CompanyPersistence::delete(&persistence, company_id).await;
}

/// Taking an address an allowlist already named keeps it on the allowlist, keeps its access and
/// keeps it in the threads it was party to -- what adopting the outsider would have kept.
#[tokio::test]
async fn account_email_change_adopts_an_allowlisted_outsider_address() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let owner = verified_account(&persistence, "lister").await;
    let member = verified_account(&persistence, "listed").await;
    let company_id = company_of(&persistence, &owner, "allow").await;
    join(&persistence, company_id, &member).await;
    let home = format!("{}@home.test", unique("home"));
    let channel = ChannelPersistence::create(
        &persistence,
        company_id,
        ChannelWrite {
            name: "Private".into(),
            slug: "private".into(),
            participant_emails: Some(vec![home.clone()]),
            enabled: true,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let outsider = access(&persistence, company_id, &home)
        .await
        .principal_id
        .unwrap()
        .as_uuid();
    let thread_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO threads (id, company_id, channel_id, subject) VALUES ($1, $2, $3, 'Hi')",
    )
    .bind(thread_id)
    .bind(company_id)
    .bind(channel.id)
    .execute(persistence.pool())
    .await
    .unwrap();
    sqlx::query(
        r#"INSERT INTO thread_principals (company_id, channel_id, thread_id, principal_id, role)
           VALUES ($1, $2, $3, $4, 'author')"#,
    )
    .bind(company_id)
    .bind(channel.id)
    .bind(thread_id)
    .bind(outsider)
    .execute(persistence.pool())
    .await
    .unwrap();
    let sent = mail_from(&persistence, company_id, &home, &owner.email).await;
    let person = person_principal(&persistence, company_id, &member).await;

    change_address(&persistence, &member, &home, AddressChange::Confirmed).await;

    let after = ChannelPersistence::get_by_id(&persistence, channel.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        after.participant_emails,
        Some(vec![EmailAddress::from(home.as_str())]),
        "the allowlist still reads back the address it was given"
    );
    assert!(
        after
            .principal_grants
            .iter()
            .all(|grant| grant.principal_id.as_uuid() == person),
        "the grants now hang on the person"
    );
    assert_eq!(after.principal_grants.len(), 2);
    let home_access = access(&persistence, company_id, &home).await;
    assert_eq!(
        home_access.principal_id.map(|id| id.as_uuid()),
        Some(person)
    );
    assert_eq!(home_access.membership, CompanyMembership::Member);
    assert_eq!(message_author(&persistence, sent).await.0, person);

    let thread_parties: Vec<Uuid> =
        sqlx::query_scalar("SELECT principal_id FROM thread_principals WHERE thread_id = $1")
            .bind(thread_id)
            .fetch_all(persistence.pool())
            .await
            .unwrap();
    assert_eq!(thread_parties, vec![person]);
    let leftovers: (i64, i64) = sqlx::query_as(
        r#"SELECT (SELECT COUNT(*) FROM participant_identities WHERE principal_id = $1),
                  (SELECT COUNT(*) FROM channel_principal_grants WHERE principal_id = $1)"#,
    )
    .bind(outsider)
    .fetch_one(persistence.pool())
    .await
    .unwrap();
    assert_eq!(leftovers, (0, 0), "the outsider is left unreachable");

    let _ = CompanyPersistence::delete(&persistence, company_id).await;
}

/// An address held by another account's person principal is not taken over; the change fails and
/// leaves everything as it was.
#[tokio::test]
async fn account_email_change_refuses_an_address_another_person_holds() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let owner = verified_account(&persistence, "holder").await;
    let mover = verified_account(&persistence, "taker").await;
    let company_id = company_of(&persistence, &owner, "held").await;
    join(&persistence, company_id, &mover).await;
    // A stale person identity for an address no account row holds any more.
    let stale = format!("{}@example.test", unique("stale"));
    sqlx::query(
        "UPDATE participant_identities SET subject = $2 WHERE company_id = $1 AND subject = $3",
    )
    .bind(company_id)
    .bind(&stale)
    .bind(owner.email.to_lowercase())
    .execute(persistence.pool())
    .await
    .unwrap();

    let refused = persistence
        .update_profile(
            mover.id,
            ProfileUpdate {
                username: &mover.username,
                email: &EmailAddress::from(stale.as_str()),
                avatar_url: None,
            },
        )
        .await;
    assert!(matches!(refused, Err(AppError::Conflict(_))));
    assert_eq!(
        UserPersistence::get_by_id(&persistence, mover.id)
            .await
            .unwrap()
            .unwrap()
            .email,
        mover.email,
        "the address change rolled back with the re-key"
    );
    assert_eq!(
        access(&persistence, company_id, &mover.email)
            .await
            .membership,
        CompanyMembership::Member
    );

    let _ = CompanyPersistence::delete(&persistence, company_id).await;
}

/// Competing claimants: an invite acceptance for the old address races the address change. Either
/// the acceptance saw the new address and is refused, or it committed first and the re-key moved
/// the principal it created. In no outcome does the old address keep membership.
#[tokio::test]
async fn account_email_change_races_invite_acceptance() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = Arc::new(PostgresPersistence::new(pool));
    for _ in 0..4 {
        let user = verified_account(&persistence, "racer").await;
        let host = verified_account(&persistence, "racehost").await;
        let company_id = company_of(&persistence, &host, "race").await;
        let invite = persistence
            .create_invite(company_id, &user.email, CompanyAccessRole::Member)
            .await
            .unwrap();
        let old = user.email.clone();
        let new = format!("{}@example.test", unique("raced"));

        let acceptance = {
            let persistence = persistence.clone();
            let user = user.clone();
            tokio::spawn(async move {
                persistence
                    .accept_pending_invite(invite.id, user.id, &user.email)
                    .await
            })
        };
        let change = {
            let persistence = persistence.clone();
            let user = user.clone();
            let new = new.clone();
            tokio::spawn(async move {
                change_address(&persistence, &user, &new, AddressChange::Bypass).await
            })
        };
        let accepted = acceptance.await.unwrap().unwrap();
        change.await.unwrap();

        let old_access = access(&persistence, company_id, &old).await;
        assert_eq!(old_access.membership, CompanyMembership::None);
        let new_access = access(&persistence, company_id, &new).await;
        if accepted.is_some() {
            assert_eq!(new_access.membership, CompanyMembership::Member);
            assert_eq!(
                new_access.principal_id.map(|id| id.as_uuid()),
                Some(person_principal(&persistence, company_id, &user).await)
            );
        } else {
            assert!(new_access.principal_id.is_none(), "nothing was registered");
        }
        let _ = CompanyPersistence::delete(persistence.as_ref(), company_id).await;
    }
}

/// Competing claimants: mail from the new and the old address is observed while the address
/// changes. The identity lock serializes them, so each address ends with exactly one principal --
/// the person for the new one, an outsider for the old one -- and no duplicate is left behind.
#[tokio::test]
async fn account_email_change_races_inbound_observation() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = Arc::new(PostgresPersistence::new(pool));
    for _ in 0..4 {
        let user = verified_account(&persistence, "observed").await;
        let company_id = company_of(&persistence, &user, "observe").await;
        let old = user.email.clone();
        let new = format!("{}@example.test", unique("seen"));

        let mut racers = tokio::task::JoinSet::new();
        for address in [new.clone(), old.clone(), new.clone(), old.clone()] {
            let persistence = persistence.clone();
            racers.spawn(async move {
                persistence
                    .resolve_or_create_external_identity(
                        company_id,
                        IdentityObservation {
                            identity: email_identity(&address),
                            display_label: None,
                            claim_metadata: IdentityClaimMetadata::observation(),
                            provenance: IdentityProvenance::TransportIngress,
                        },
                    )
                    .await
                    .map(|_| ())
            });
        }
        {
            let persistence = persistence.clone();
            let user = user.clone();
            let new = new.clone();
            racers.spawn(async move {
                change_address(&persistence, &user, &new, AddressChange::Bypass).await;
                Ok(())
            });
        }
        while let Some(joined) = racers.join_next().await {
            joined.unwrap().unwrap();
        }

        let person = person_principal(&persistence, company_id, &user).await;
        let new_access = access(&persistence, company_id, &new).await;
        assert_eq!(new_access.principal_id.map(|id| id.as_uuid()), Some(person));
        assert_eq!(new_access.membership, CompanyMembership::Owner);
        let old_access = access(&persistence, company_id, &old).await;
        assert_eq!(old_access.membership, CompanyMembership::None);
        assert_eq!(
            principal_kind(&persistence, old_access.principal_id.unwrap().as_uuid()).await,
            "external"
        );
        let rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM participant_identities WHERE company_id = $1 AND subject = ANY($2)",
        )
        .bind(company_id)
        .bind(vec![old.to_lowercase(), new.to_lowercase()])
        .fetch_one(persistence.pool())
        .await
        .unwrap();
        assert_eq!(rows, 2, "one identity row per address");
        let _ = CompanyPersistence::delete(persistence.as_ref(), company_id).await;
    }
}

/// Registration accepts any plausible address, including ones no identity can be written for. An
/// account with such an address and no company can still change it: there is nothing to re-key.
#[tokio::test]
async fn account_email_change_with_an_unparseable_old_address_and_no_company_succeeds() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let label = unique("odd");
    let odd = format!("{label},x@example.test");
    assert!(
        EmailIdentity::parse(EmailAddress::from(odd.as_str())).is_err(),
        "the fixture address must be one the identity parser rejects"
    );
    let user = persistence.create_user(&label, &odd, "hash").await.unwrap();
    let new = format!("{label}@example.test");

    let changed = change_address(&persistence, &user, &new, AddressChange::Bypass).await;
    assert_eq!(changed.email, new);
}
