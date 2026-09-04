//! The fixture the two database-test modules in this directory share.
//!
//! They run against a live database and skip when there is none, matching the rest of the
//! persistence suite. Every count and lookup is scoped to the fixture's own company: the suite
//! shares one database and runs in parallel, so a whole-table assertion would be a coin toss.

use super::*;
use crate::adapters::persistence::test_support::test_pool;
use crate::entities::{
    correlation::CorrelationId,
    email_message::EmailMessageMetadata,
    message::{MessageDirection, MessageParticipantKind, MessageRole},
    participant::{IdentityClaimMetadata, IdentityProvenance},
    transport::ChannelBindingId,
    value_objects::MessageId,
};
use crate::use_cases::{
    channel::{ChannelPersistence, ChannelWrite},
    company::{CompanyPersistence, CompanyWrite},
    participant::IdentityObservation,
    thread::{MessageAuthorWrite, MessageCorrelation, MessageParticipantWrite},
    user::UserPersistence,
};
use sqlx::PgPool;

/// A company with one channel and one thread, plus the handles the tests write messages with.
pub(super) struct Fixture {
    pub(super) persistence: PostgresPersistence,
    pub(super) pool: PgPool,
    pub(super) company_id: Uuid,
    pub(super) owner_id: Uuid,
    pub(super) channel_id: Uuid,
    pub(super) thread: Thread,
    pub(super) suffix: String,
}

impl Fixture {
    /// `None` when there is no database to talk to.
    pub(super) async fn new(label: &str) -> Option<Self> {
        let pool = test_pool().await?;
        let persistence = PostgresPersistence::new(pool.clone());

        let suffix = Uuid::new_v4().simple().to_string();
        let username = format!("{label}_{suffix}");
        let email = format!("{username}@example.com");
        persistence
            .create_user(&username, &email, "hash")
            .await
            .unwrap();
        let owner = UserPersistence::get_by_email(&persistence, &email)
            .await
            .unwrap()
            .unwrap();
        let company = CompanyPersistence::create(
            &persistence,
            owner.id,
            CompanyWrite {
                name: "Canonical Messages".to_string(),
                // Slugs are hyphen-only; the label reads as a Rust identifier.
                slug: format!("{}-{suffix}", label.replace('_', "-")),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();
        let channel = persistence
            .channel(company.id, "primary", &suffix)
            .await
            .unwrap();
        let thread = persistence
            .create_thread(channel.id, "Subject", &[EmailAddress::from(email)])
            .await
            .unwrap();

        Some(Self {
            persistence,
            pool,
            company_id: company.id,
            owner_id: owner.id,
            channel_id: channel.id,
            thread,
            suffix,
        })
    }

    pub(super) async fn extra_thread(&self, channel_id: Uuid, subject: &str) -> Thread {
        self.persistence
            .create_thread(
                channel_id,
                subject,
                &[EmailAddress::from("someone@partner.test")],
            )
            .await
            .unwrap()
    }

    pub(super) async fn extra_channel(&self, slug: &str) -> Uuid {
        self.persistence
            .channel(self.company_id, slug, &self.suffix)
            .await
            .unwrap()
            .id
    }

    /// A second company owned by the same user, for the cross-tenant refusals.
    pub(super) async fn foreign_company(&self) -> Uuid {
        CompanyPersistence::create(
            &self.persistence,
            self.owner_id,
            CompanyWrite {
                name: "Foreign".to_string(),
                slug: format!("foreign-{}", self.suffix),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap()
        .id
    }

    /// The channel's canonical email binding: what mail on this channel is correlated against.
    pub(super) async fn email_binding(&self) -> Uuid {
        sqlx::query_scalar(
            "SELECT id FROM channel_bindings WHERE channel_id = $1 AND transport = 'email'",
        )
        .bind(self.channel_id)
        .fetch_one(&self.pool)
        .await
        .unwrap()
    }

    /// Any channel's canonical email binding, for the multi-channel correlation cases.
    pub(super) async fn email_binding_of(&self, channel_id: Uuid) -> ChannelBindingId {
        let id: Uuid = sqlx::query_scalar(
            "SELECT id FROM channel_bindings WHERE channel_id = $1 AND transport = 'email'",
        )
        .bind(channel_id)
        .fetch_one(&self.pool)
        .await
        .unwrap();
        ChannelBindingId::new(id)
    }

    /// A second, differently-transported binding on the same channel, so a thread can be reached
    /// through more than one provider at once.
    pub(super) async fn slack_binding(&self, conversation: &str) -> ChannelBindingId {
        let installation_id: Uuid = sqlx::query_scalar(
            r#"INSERT INTO integration_installations (
                    id, company_id, transport, external_tenant_key, display_name, status,
                    installed_by, updated_by
               ) VALUES ($1, $2, 'slack', $3, 'Workspace', 'active', $4, $4)
               ON CONFLICT (transport, external_tenant_key) DO UPDATE SET status = 'active'
               RETURNING id"#,
        )
        .bind(Uuid::new_v4())
        .bind(self.company_id)
        .bind(format!("T{}", self.suffix))
        .bind(serde_json::json!({
            "actor_type": "system", "actor_id": null, "actor_name": "test"
        }))
        .fetch_one(&self.pool)
        .await
        .unwrap();

        let binding_id: Uuid = sqlx::query_scalar(
            r#"INSERT INTO channel_bindings (
                    id, company_id, channel_id, installation_id, transport, namespace,
                    external_endpoint_key, display_label, access_policy, delivery_policy, status,
                    created_by, access_snapshot
               ) VALUES ($1, $2, $3, $4, 'slack', $5, $6, 'Slack',
                         'conversation_members_read_and_participate', 'reply_only', 'active',
                         $7, $8)
               RETURNING id"#,
        )
        .bind(Uuid::new_v4())
        .bind(self.company_id)
        .bind(self.channel_id)
        .bind(installation_id)
        .bind(format!("T{}", self.suffix))
        .bind(conversation)
        .bind(serde_json::json!({
            "actor_type": "system", "actor_id": null, "actor_name": "test"
        }))
        .bind(serde_json::json!({
            "version": 1, "kind": "provider_conversation"
        }))
        .fetch_one(&self.pool)
        .await
        .unwrap();
        ChannelBindingId::new(binding_id)
    }

    /// An agent in the fixture's company, for a message authored by one.
    ///
    /// Created through the ordinary path so it gets its principal the way a real agent does; a
    /// hand-written `principals` row would not exercise the author resolution at all.
    pub(super) async fn agent(&self) -> Uuid {
        crate::use_cases::agent::AgentPersistence::create(
            &self.persistence,
            self.company_id,
            crate::use_cases::agent::AgentWrite {
                name: "Triage Agent".to_string(),
                slug: format!("triage-{}", self.suffix),
                ..crate::use_cases::agent::AgentWrite::default()
            },
        )
        .await
        .unwrap()
        .id
    }

    pub(super) async fn cleanup(&self) {
        let _ = CompanyPersistence::delete(&self.persistence, self.company_id).await;
    }
}

/// Channel creation, without repeating the whole `ChannelWrite` at every call site.
pub(super) trait TestChannel {
    async fn channel(
        &self,
        company_id: Uuid,
        slug: &str,
        suffix: &str,
    ) -> AppResult<crate::entities::channel::Channel>;
}

impl TestChannel for PostgresPersistence {
    async fn channel(
        &self,
        company_id: Uuid,
        slug: &str,
        suffix: &str,
    ) -> AppResult<crate::entities::channel::Channel> {
        ChannelPersistence::create(
            self,
            company_id,
            ChannelWrite {
                name: slug.to_string(),
                slug: format!("{slug}-{suffix}"),
                enabled: false,
                ..ChannelWrite::default()
            },
        )
        .await
    }
}

pub(super) fn observed(address: &str, provenance: IdentityProvenance) -> MessageAuthorWrite {
    MessageAuthorWrite::Observed(IdentityObservation {
        identity: crate::use_cases::thread::qualified_email_identity(address).unwrap(),
        display_label: None,
        claim_metadata: IdentityClaimMetadata::observation(),
        provenance,
    })
}

pub(super) fn participant(kind: MessageParticipantKind, address: &str) -> MessageParticipantWrite {
    MessageParticipantWrite::new(
        kind,
        crate::use_cases::thread::qualified_email_identity(address).unwrap(),
    )
}

/// One arriving email, as the ingest path states it.
pub(super) fn inbound_email(
    thread_id: Uuid,
    metadata: EmailMessageMetadata,
    body: &str,
) -> MessageWrite {
    MessageWrite {
        id: CanonicalMessageId::random(),
        thread_id,
        author: observed("sender@partner.test", IdentityProvenance::TransportIngress),
        subject: "Subject".into(),
        clean_text_body: body.into(),
        attachments: Vec::new(),
        direction: MessageDirection::Inbound,
        role: MessageRole::Human,
        correlation_id: CorrelationId::new(),
        participants: vec![
            participant(MessageParticipantKind::Sender, "sender@partner.test"),
            participant(MessageParticipantKind::To, "primary@example.com"),
            participant(MessageParticipantKind::Cc, "watcher@partner.test"),
        ],
        correlation: MessageCorrelation::Email(metadata),
        created_at: Utc::now(),
    }
}

pub(super) fn email_metadata(rfc: &str) -> EmailMessageMetadata {
    EmailMessageMetadata::new(MessageId::from(rfc)).raw_bodies(Some("raw".into()), None)
}
