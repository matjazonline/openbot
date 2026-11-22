//! `address_resolution_`: an addressed local part resolves through the company's one address
//! namespace to a desk, a principal, a system address or nothing.
use super::*;
use crate::application::ports::channel::ChannelUpdate;
use crate::entities::address::{AddressTarget, ResolvedAddress};

/// The fixture's desks, plus principal addresses at these slugs in its company.
struct NamespaceChannels {
    desks: Arc<dyn ChannelPersistence>,
    company_id: Uuid,
    principals: Vec<ResolvedAddress>,
}

#[async_trait]
impl ChannelPersistence for NamespaceChannels {
    async fn create(
        &self,
        actor: crate::application::workflow::WorkflowActor,
        company_id: Uuid,
        write: ChannelWrite,
    ) -> AppResult<Channel> {
        self.desks.create(actor, company_id, write).await
    }

    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Channel>> {
        self.desks.get_by_id(id).await
    }

    async fn get_by_company_slug_and_channel_slug(
        &self,
        company_slug: &CompanySlug,
        channel_slug: &ChannelSlug,
    ) -> AppResult<Option<Channel>> {
        self.desks
            .get_by_company_slug_and_channel_slug(company_slug, channel_slug)
            .await
    }

    async fn list_by_company_id(&self, company_id: Uuid) -> AppResult<Vec<Channel>> {
        self.desks.list_by_company_id(company_id).await
    }

    async fn address_for_target(
        &self,
        company_id: Uuid,
        target: AddressTarget,
    ) -> AppResult<Option<ResolvedAddress>> {
        self.desks.address_for_target(company_id, target).await
    }

    async fn address_targets(
        &self,
        company_id: Uuid,
        slugs: &[ChannelSlug],
    ) -> AppResult<Vec<ResolvedAddress>> {
        let mut resolved = self.desks.address_targets(company_id, slugs).await?;
        if company_id == self.company_id {
            resolved.extend(
                self.principals
                    .iter()
                    .filter(|address| slugs.contains(&address.slug))
                    .cloned(),
            );
        }
        Ok(resolved)
    }

    async fn list_departments(
        &self,
        company_id: Uuid,
        archived: crate::entities::department::ArchivedUnits,
    ) -> AppResult<Vec<crate::entities::department::Department>> {
        self.desks.list_departments(company_id, archived).await
    }

    async fn update(&self, request: ChannelUpdate) -> AppResult<Channel> {
        self.desks.update(request).await
    }

    async fn delete(
        &self,
        actor: crate::application::workflow::WorkflowActor,
        company_id: Uuid,
        id: Uuid,
    ) -> AppResult<()> {
        self.desks.delete(actor, company_id, id).await
    }
}

/// The plain `support` desk fixture, with a person at `alice` in the same company.
fn fixture() -> ChannelFixture {
    let mut fixture = channel_fixture(TestChannel::default());
    let target =
        crate::use_cases::participant::test_support::principal_for_email(fixture.company_id, TEAM);
    let owner = Uuid::new_v4();
    let principal = crate::entities::participant::Principal {
        id: target,
        company_id: fixture.company_id,
        kind: crate::entities::participant::PrincipalKind::Person,
        user_id: Some(owner),
        agent_id: None,
        display_label: "Alice".into(),
        open_to_public: false,
        settings_revision: 1,
        historical_person_user_id: Some(owner),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    fixture.participants.insert_principal(principal.clone());
    fixture.participants.insert_direct_inbox(
        owner,
        crate::application::ports::participant::DirectInbox {
            principal,
            slug: Some("alice".into()),
        },
    );
    let channels = Arc::new(NamespaceChannels {
        desks: fixture.use_cases.channel_persistence.clone(),
        company_id: fixture.company_id,
        principals: vec![ResolvedAddress {
            slug: "alice".into(),
            target: AddressTarget::Principal(target),
        }],
    });
    fixture.use_cases = ThreadUseCases::for_test(
        fixture.threads.clone(),
        channels,
        fixture.use_cases.company_persistence.clone(),
        fixture.participants.clone(),
        internal_test_settings(),
    );
    fixture
}

async fn ingest(fixture: &ChannelFixture, from: &str, to: &str) -> InboundIngestResult {
    fixture
        .use_cases
        .ingest_test_email(RawInboundPayload {
            to: to.into(),
            from: from.into(),
            subject: Some("Hello".into()),
            text: Some("Hello there".into()),
            ..Default::default()
        })
        .await
        .unwrap()
}

const TEAM: &str = "team@acme.com";
const OUTSIDER: &str = "stranger@elsewhere.test";

#[tokio::test]
async fn address_resolution_a_desk_and_the_senders_direct_address_are_admitted() {
    let fixture = fixture();
    let desk = ingest(&fixture, TEAM, "support@acme.mailagents.com").await;
    assert!(desk.accepted, "{:?}", desk.reason());

    let person = ingest(&fixture, TEAM, "alice@acme.mailagents.com").await;
    assert!(person.accepted, "{:?}", person.reason());
    assert!(matches!(
        person.thread.unwrap().target,
        AddressTarget::Principal(_)
    ));
    assert_eq!(person.intent_ids.len(), 1);
}

#[tokio::test]
async fn address_resolution_a_context_suffix_strips_for_a_desk_and_is_refused_for_a_person() {
    let fixture = fixture();
    let desk = ingest(&fixture, TEAM, "support+quiet@acme.mailagents.com").await;
    assert!(desk.accepted, "{:?}", desk.reason());

    let person = ingest(&fixture, TEAM, "alice+quiet@acme.mailagents.com").await;
    assert!(!person.accepted);
    assert_eq!(
        person.reason(),
        Some("A context suffix is not accepted on a person's or an agent's address")
    );
    assert_eq!(
        person.bounce_info().unwrap().principals[0].refusal,
        PrincipalRefusal::ContextSuffix
    );
}

#[tokio::test]
async fn address_resolution_a_desk_and_a_person_together_get_one_copy_each() {
    let fixture = fixture();
    let mixed = ingest(&fixture, TEAM, "support+alice@acme.mailagents.com").await;
    assert!(mixed.accepted, "{:?}", mixed.reason());
    assert_eq!(mixed.channel_matches.len(), 2);
    assert_eq!(mixed.intent_ids.len(), 2);
    assert!(matches!(
        mixed.channel_matches[0].thread.target,
        AddressTarget::Desk(_)
    ));
    assert!(matches!(
        mixed.channel_matches[1].thread.target,
        AddressTarget::Principal(_)
    ));
}

#[tokio::test]
async fn address_resolution_a_stranger_cannot_tell_a_person_from_nobody() {
    let fixture = fixture();
    for to in ["alice", "alice+quiet", "support+alice"] {
        let person = ingest(&fixture, OUTSIDER, &format!("{to}@acme.mailagents.com")).await;
        let nobody = ingest(
            &fixture,
            OUTSIDER,
            &format!("{}@acme.mailagents.com", to.replace("alice", "nobody")),
        )
        .await;
        // Same kind, sentence, suggestions and bounce; only the slug the stranger typed and the
        // per-message key differ.
        let key = person.bounce_info().unwrap().source_message_key.clone();
        assert_eq!(
            person.rejection,
            nobody
                .rejection
                .as_ref()
                .map(|rejection| rejection_with_slug(rejection, "nobody", "alice", &key)),
            "{to}"
        );
        let person_bounce = person.bounce_info().unwrap();
        assert!(person_bounce.principals.is_empty(), "{to}");
        assert_eq!(
            format_bounce_email_body(person_bounce, "mailagents.com"),
            format_bounce_email_body(nobody.bounce_info().unwrap(), "mailagents.com")
                .replace("nobody", "alice"),
            "{to}"
        );
    }
}

/// A rejection with every occurrence of one typed slug swapped for another, under `key`.
fn rejection_with_slug(
    rejection: &IngestRejection,
    from: &str,
    to: &str,
    key: &ExternalMessageKey,
) -> IngestRejection {
    let mut rejection = rejection.clone();
    if let IngestRejection::Undeliverable(reason) = &mut rejection {
        reason.bounce.source_message_key = key.clone();
        let swap = |slug: &mut ChannelSlug| {
            if slug.as_str() == from {
                *slug = to.into();
            }
        };
        reason.bounce.invalid_slugs.iter_mut().for_each(swap);
        for suggestion in &mut reason.bounce.suggestions {
            swap(&mut suggestion.invalid_slug);
        }
    }
    rejection
}

#[tokio::test]
async fn address_resolution_an_internal_relay_to_a_person_is_refused_definitely() {
    let fixture = fixture();
    let disposition = relay_to(&fixture, "alice").await;
    assert!(
        matches!(
            &disposition,
            crate::transport::RelayDisposition::Refused(reason)
                if reason == "Channel address not found or misspelled"
        ),
        "{disposition:?}"
    );
}

/// One internal hop from the fixture's `support` desk to `{slug}` in the same company.
async fn relay_to(fixture: &ChannelFixture, slug: &str) -> crate::transport::RelayDisposition {
    let hop = InternalHop::new(
        fixture.channel_id,
        "support@acme.mailagents.com",
        &format!("{slug}@acme.mailagents.com"),
        "Hand-off",
        "Please take this",
    );
    relay_hop(&fixture.use_cases, &hop).await
}

#[tokio::test]
async fn direct_trace_principal_cycle_is_refused_without_conflating_a_desk_with_the_same_uuid() {
    use crate::application::ports::participant::PrincipalAccessPersistence;
    let f = fixture();
    let id = crate::use_cases::participant::test_support::principal_for_email(f.company_id, TEAM);
    let mut target = f
        .participants
        .principal_by_id(f.company_id, id)
        .await
        .unwrap()
        .unwrap();
    // Named public-agent stand-in; production registration and sender identity remain p03 proof.
    target.kind = crate::entities::participant::PrincipalKind::Agent;
    target.agent_id = Some(Uuid::new_v4());
    target.user_id = None;
    target.historical_person_user_id = None;
    target.open_to_public = true;
    f.participants.insert_principal(target);
    let mut hop = InternalHop::new(
        f.channel_id,
        "support@acme.mailagents.com",
        "alice@acme.mailagents.com",
        "Typed trace",
        "Please take this",
    );
    hop.trace_targets = vec![AddressTarget::Principal(id)];
    let cycle = relay_hop(&f.use_cases, &hop).await;
    assert_eq!(refusal(&cycle), Some(IngestRejection::LoopCycle.as_str()));
    assert!(f.threads.threads().is_empty(), "cycle creates no thread");
    hop.trace_targets = vec![AddressTarget::Desk(id.as_uuid())];
    assert!(matches!(
        relay_hop(&f.use_cases, &hop).await,
        crate::transport::RelayDisposition::Relayed
    ));
    assert_eq!(f.threads.threads().len(), 1);
    assert!(f.threads.threads()[0].belongs_to(f.company_id, AddressTarget::Principal(id)));
}
