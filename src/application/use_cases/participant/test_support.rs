//! One in-memory identity directory shared by every test that needs principals.
//!
//! Principal ids are *derived* from the qualified identity rather than allocated, so a test can
//! write a channel grant or a thread participant for an address before anything has observed it —
//! which is what lets fixture channels stay plain data instead of needing a live directory.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use chrono::Utc;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    adapters::protocols::email::EmailIdentity,
    app_error::{AppError, AppResult},
    entities::{
        channel::{ChannelAccessMode, PUBLIC_PARTICIPANT},
        company::CompanyAccess,
        company_member::CompanyMembership,
        participant::{
            ChannelPrincipalGrant, GrantProvenance, IdentityProvenance, IdentityStatus,
            ParticipantIdentity, Principal, PrincipalAccessContext, PrincipalCapability,
            PrincipalKind,
        },
        transport::{ParticipantIdentityId, PrincipalId, QualifiedIdentity, TransportKind},
        value_objects::EmailAddress,
    },
    use_cases::participant::{
        IdentityDirectory, IdentityObservation, IdentityResolution, PrincipalAccessPersistence,
    },
};

/// The principal a qualified identity resolves to inside this directory.
///
/// A digest of the tenant-qualified key, so the mapping is stable across runs and across the two
/// halves of a test that seed a grant and then send a message.
pub fn principal_for_identity(company_id: Uuid, identity: &QualifiedIdentity) -> PrincipalId {
    let mut digest = Sha256::new();
    digest.update(company_id.as_bytes());
    digest.update(b":");
    digest.update(identity.transport().as_str().as_bytes());
    digest.update(b":");
    digest.update(identity.namespace().as_str().as_bytes());
    digest.update(b":");
    digest.update(identity.subject().as_str().as_bytes());
    let bytes: [u8; 16] = digest.finalize()[..16]
        .try_into()
        .expect("sha256 yields at least 16 bytes");
    PrincipalId::new(Uuid::from_bytes(bytes))
}

/// The principal an email address resolves to. Panics on an unparseable address, because a test
/// fixture that cannot name its own actor has a typo rather than a scenario.
pub fn principal_for_email(company_id: Uuid, address: &str) -> PrincipalId {
    let identity = EmailIdentity::parse(EmailAddress::from(address))
        .expect("test fixture email address is parseable")
        .qualify_default();
    principal_for_identity(company_id, &identity)
}

/// The access policy a channel form's email allowlist produces, mirroring `channel_access` and
/// `insert_email_allowlist_grants` in the persistence writer: every entered address gets both
/// capabilities, and `@public` is an access *mode* on the channel rather than a grant.
pub fn email_allowlist_policy(
    company_id: Uuid,
    participant_emails: Option<&[EmailAddress]>,
) -> (ChannelAccessMode, Vec<ChannelPrincipalGrant>) {
    let listed: Vec<&str> = participant_emails
        .unwrap_or_default()
        .iter()
        .map(|email| email.trim())
        .filter(|email| !email.is_empty())
        .collect();
    let is_public = listed
        .iter()
        .any(|email| email.eq_ignore_ascii_case(PUBLIC_PARTICIPANT));
    let grants = email_allowlist_grants(company_id, &listed);
    let mode = if is_public {
        ChannelAccessMode::Public
    } else if grants.is_empty() {
        ChannelAccessMode::Team
    } else {
        ChannelAccessMode::Allowlist
    };
    (mode, grants)
}

/// The grants alone, for a fixture that already states its access mode literally.
pub fn email_allowlist_grants(company_id: Uuid, addresses: &[&str]) -> Vec<ChannelPrincipalGrant> {
    let mut seen = HashSet::new();
    addresses
        .iter()
        .map(|address| address.trim().to_lowercase())
        .filter(|address| {
            !address.is_empty()
                && !address.eq_ignore_ascii_case(PUBLIC_PARTICIPANT)
                && seen.insert(address.clone())
        })
        .flat_map(|address| {
            let principal_id = principal_for_email(company_id, &address);
            [PrincipalCapability::Participate, PrincipalCapability::View].map(|capability| {
                ChannelPrincipalGrant {
                    principal_id,
                    capability,
                    provenance: GrantProvenance::EmailAllowlist,
                    created_at: Utc::now(),
                }
            })
        })
        .collect()
}

/// How a test states who is on a company's team.
///
/// Production answers this by joining a principal to its `company_members` row, inside the same
/// query that resolves the identity. An in-memory directory has no join to make, so the scenario's
/// own company double answers instead — which keeps the team stated once, where the test already
/// listed it. It is deliberately test-only: nothing in production decides anything from an address.
#[async_trait]
pub trait TeamFixture: Send + Sync {
    /// What the account behind this address is to the company.
    async fn membership_for_email(
        &self,
        company_id: Uuid,
        email: &str,
    ) -> AppResult<CompanyMembership>;

    /// What this signed-in account is to the company.
    async fn company_access(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Option<CompanyAccess>>;
}

#[derive(Default)]
struct DirectoryState {
    identities: HashMap<(Uuid, QualifiedIdentity), ParticipantIdentity>,
    users: HashMap<(Uuid, Uuid), PrincipalId>,
}

/// An `IdentityDirectory` + `PrincipalAccessPersistence` backed by a `HashMap`.
///
/// Identities and principals live here; membership comes from the scenario's [`TeamFixture`], so
/// a test states its team once and both halves of an access context agree.
pub struct InMemoryParticipantDirectory {
    team: Option<Arc<dyn TeamFixture>>,
    state: Mutex<DirectoryState>,
}

impl Default for InMemoryParticipantDirectory {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryParticipantDirectory {
    pub fn new() -> Self {
        Self {
            team: None,
            state: Mutex::new(DirectoryState::default()),
        }
    }

    /// Answer memberships out of the same company double the rest of the test uses. Without one,
    /// every resolved actor is an outsider.
    pub fn with_team(mut self, team: Arc<dyn TeamFixture>) -> Self {
        self.team = Some(team);
        self
    }

    /// Attach an account to its company principal, the way company/invite bootstrap does.
    pub fn with_user(self, company_id: Uuid, user_id: Uuid, email: &str) -> Self {
        let principal_id = principal_for_email(company_id, email);
        self.state
            .lock()
            .unwrap()
            .users
            .insert((company_id, user_id), principal_id);
        self
    }

    /// Number of identities actually observed by a write. Policy tests use this to prove a
    /// rejected ingress performed only lookups.
    pub fn identity_count(&self) -> usize {
        self.state.lock().unwrap().identities.len()
    }

    async fn membership_for_subject(
        &self,
        company_id: Uuid,
        identity: &QualifiedIdentity,
    ) -> AppResult<CompanyMembership> {
        if identity.transport() != TransportKind::Email {
            return Ok(CompanyMembership::None);
        }
        match &self.team {
            Some(team) => {
                team.membership_for_email(company_id, identity.subject().as_str())
                    .await
            }
            None => Ok(CompanyMembership::None),
        }
    }

    fn upsert(
        &self,
        company_id: Uuid,
        identity: &QualifiedIdentity,
        display_label: Option<String>,
        provenance: IdentityProvenance,
        claim_metadata: crate::entities::participant::IdentityClaimMetadata,
    ) -> ParticipantIdentity {
        let mut state = self.state.lock().unwrap();
        state
            .identities
            .entry((company_id, identity.clone()))
            .or_insert_with(|| ParticipantIdentity {
                id: ParticipantIdentityId::random(),
                company_id,
                principal_id: principal_for_identity(company_id, identity),
                transport: identity.transport(),
                namespace: identity.namespace().clone(),
                subject: identity.subject().clone(),
                display_label,
                status: IdentityStatus::Observed,
                claim_metadata,
                provenance,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            })
            .clone()
    }

    fn principal_from(
        &self,
        identity: &ParticipantIdentity,
        membership: CompanyMembership,
    ) -> Principal {
        Principal {
            id: identity.principal_id,
            company_id: identity.company_id,
            kind: if membership.is_team() {
                PrincipalKind::Person
            } else {
                PrincipalKind::External
            },
            user_id: None,
            agent_id: None,
            display_label: identity
                .display_label
                .clone()
                .unwrap_or_else(|| identity.subject.as_str().to_string()),
            created_at: identity.created_at,
            updated_at: identity.updated_at,
        }
    }
}

#[async_trait]
impl IdentityDirectory for InMemoryParticipantDirectory {
    async fn resolve_or_create_external_identity(
        &self,
        company_id: Uuid,
        observation: IdentityObservation,
    ) -> AppResult<IdentityResolution> {
        let identity = self.upsert(
            company_id,
            &observation.identity,
            observation.display_label,
            observation.provenance,
            observation.claim_metadata,
        );
        let membership = self
            .membership_for_subject(company_id, &observation.identity)
            .await?;
        Ok(IdentityResolution {
            principal: self.principal_from(&identity, membership),
            identity,
        })
    }

    async fn identities_for_principals(
        &self,
        company_id: Uuid,
        principal_ids: &[PrincipalId],
        transport: TransportKind,
    ) -> AppResult<Vec<ParticipantIdentity>> {
        let state = self.state.lock().unwrap();
        let mut found: Vec<ParticipantIdentity> = state
            .identities
            .values()
            .filter(|identity| {
                identity.company_id == company_id
                    && identity.transport == transport
                    && principal_ids.contains(&identity.principal_id)
            })
            .cloned()
            .collect();
        found.sort_by(|left, right| {
            (left.created_at, left.id.as_uuid()).cmp(&(right.created_at, right.id.as_uuid()))
        });
        Ok(found)
    }
}

#[async_trait]
impl PrincipalAccessPersistence for InMemoryParticipantDirectory {
    async fn access_context_for_identity(
        &self,
        company_id: Uuid,
        identity: &QualifiedIdentity,
    ) -> AppResult<PrincipalAccessContext> {
        let known = self
            .state
            .lock()
            .unwrap()
            .identities
            .get(&(company_id, identity.clone()))
            .cloned();
        if known
            .as_ref()
            .is_some_and(|known| known.status == IdentityStatus::Disabled)
        {
            return Ok(PrincipalAccessContext {
                principal_id: None,
                membership: CompanyMembership::None,
            });
        }
        // Test fixtures state ACL principals and team membership without running the database
        // bootstrap that would have inserted their identity rows. Derive the same stable principal
        // for a read, but do not put it in `state`: a policy lookup must remain observable as
        // read-only, and accepted-message tests still resolve the same actor their grants name.
        Ok(PrincipalAccessContext {
            principal_id: Some(
                known
                    .map(|known| known.principal_id)
                    .unwrap_or_else(|| principal_for_identity(company_id, identity)),
            ),
            membership: self.membership_for_subject(company_id, identity).await?,
        })
    }

    async fn access_context_for_user(
        &self,
        company_id: Uuid,
        user_id: Uuid,
    ) -> AppResult<Option<PrincipalAccessContext>> {
        let principal_id = self
            .state
            .lock()
            .unwrap()
            .users
            .get(&(company_id, user_id))
            .copied();
        let Some(principal_id) = principal_id else {
            return Ok(None);
        };
        let membership = match &self.team {
            Some(team) => team
                .company_access(user_id, company_id)
                .await?
                .map(|access| access.membership)
                .unwrap_or(CompanyMembership::None),
            None => CompanyMembership::None,
        };
        Ok(Some(PrincipalAccessContext {
            principal_id: Some(principal_id),
            membership,
        }))
    }
}

/// A directory that fails every call, for tests asserting that an outage propagates instead of
/// becoming an authorization verdict.
pub struct FailingParticipantDirectory;

fn outage() -> AppError {
    AppError::Internal("participant directory is unavailable".into())
}

#[async_trait]
impl IdentityDirectory for FailingParticipantDirectory {
    async fn resolve_or_create_external_identity(
        &self,
        _company_id: Uuid,
        _observation: IdentityObservation,
    ) -> AppResult<IdentityResolution> {
        Err(outage())
    }

    async fn identities_for_principals(
        &self,
        _company_id: Uuid,
        _principal_ids: &[PrincipalId],
        _transport: TransportKind,
    ) -> AppResult<Vec<ParticipantIdentity>> {
        Err(outage())
    }
}

#[async_trait]
impl PrincipalAccessPersistence for FailingParticipantDirectory {
    async fn access_context_for_identity(
        &self,
        _company_id: Uuid,
        _identity: &QualifiedIdentity,
    ) -> AppResult<PrincipalAccessContext> {
        Err(outage())
    }

    async fn access_context_for_user(
        &self,
        _company_id: Uuid,
        _user_id: Uuid,
    ) -> AppResult<Option<PrincipalAccessContext>> {
        Err(outage())
    }
}
