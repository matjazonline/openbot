//! Relays into a members-only desk (01-departments p02, 01DEP-C001): internal relays stay trusted,
//! so a members-only desk admits desk-to-desk mail whether or not the sending desk belongs to one
//! of its department's agents, and the relay trace still refuses a loop definitely.
use super::*;
use crate::entities::department::DepartmentId;
use crate::entities::participant::{ChannelPrincipalGrant, GrantProvenance, PrincipalCapability};
use crate::entities::transport::PrincipalId;

fn desk(company_id: Uuid, slug: &str) -> Channel {
    Channel {
        enabled: true,
        add_3rd_party: true,
        id: Uuid::new_v4(),
        company_id,
        name: slug.to_string(),
        description: None,
        slug: slug.into(),
        alias_slugs: Vec::new(),
        participant_emails: None,
        access_mode: ChannelAccessMode::Team,
        principal_grants: Vec::new(),
        created_by: crate::entities::creation::CreationProvenance::system(),
        created_at: Utc::now(),
        settings_revision: 0,
        department_id: None,
        members_only: false,
    }
}

/// A members-only desk whose only grants are one member's derived ones.
fn members_only_desk(company_id: Uuid, slug: &str, member: PrincipalId) -> Channel {
    let derived = |capability| ChannelPrincipalGrant {
        principal_id: member,
        capability,
        provenance: GrantProvenance::DepartmentMembership,
        created_at: Utc::now(),
    };
    Channel {
        access_mode: ChannelAccessMode::Allowlist,
        department_id: Some(DepartmentId::random()),
        members_only: true,
        principal_grants: vec![
            derived(PrincipalCapability::Participate),
            derived(PrincipalCapability::View),
        ],
        ..desk(company_id, slug)
    }
}

#[tokio::test]
async fn department_desk_relay_into_a_members_only_desk_stays_trusted() {
    let company_id = Uuid::new_v4();
    let target = members_only_desk(company_id, "ops", PrincipalId::random());
    let member_desk = desk(company_id, "member-agent");
    let outside_desk = desk(company_id, "outside-agent");
    let (target_id, member_id, outside_id) = (target.id, member_desk.id, outside_desk.id);
    let company_persistence = Arc::new(company_double(vec![Company {
        channel_defaults: Default::default(),
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        avatar_url: None,
        created_at: Utc::now(),
    }]));
    let thread_use_cases = ThreadUseCases::for_test(
        Arc::new(InMemoryThreads::for_company(company_id)),
        Arc::new(MockChannelPersistence {
            departments: Default::default(),
            channels: Mutex::new(vec![target, member_desk, outside_desk]),
        }),
        company_persistence.clone(),
        Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
        Arc::new(ThreadSettings {
            mail_domain: ("mailagents.com".to_string()).into(),
            max_spam_score: 5.0,
        }),
    );

    for (source, from) in [
        (member_id, "member-agent@acme.mailagents.com"),
        (outside_id, "outside-agent@acme.mailagents.com"),
    ] {
        let disposition = relay_hop(
            &thread_use_cases,
            &InternalHop::new(
                source,
                from,
                "ops@acme.mailagents.com",
                "Work",
                "Please handle",
            ),
        )
        .await;
        assert_eq!(
            disposition,
            crate::transport::RelayDisposition::Relayed,
            "a relay from {from} is admitted"
        );
    }

    // A desk reappearing in the trace is still a cycle, refused definitely.
    let looped = relay_hop(
        &thread_use_cases,
        &InternalHop {
            hop_count: 2,
            trace_channels: vec![target_id, outside_id],
            ..InternalHop::new(
                outside_id,
                "outside-agent@acme.mailagents.com",
                "ops@acme.mailagents.com",
                "Back again",
                "Round we go",
            )
        },
    )
    .await;
    assert_eq!(refusal(&looped), Some("Inter-channel loop cycle detected"));
}

/// DEP-P02-D6 (F8): a members-only desk is an allowlist desk, so the existing thread-party rule
/// still lets a sender reply within a thread they are a party to after their derived grant is
/// gone; a new conversation from them is refused.
#[tokio::test]
async fn department_desk_members_only_thread_party_keeps_replying_after_removal() {
    use crate::use_cases::participant::test_support::principal_for_email;
    let company_id = Uuid::new_v4();
    let member = principal_for_email(company_id, "member@acme.com");
    let desk = members_only_desk(company_id, "ops", member);
    let company_persistence = Arc::new(company_double_with_team(
        vec![Company {
            channel_defaults: Default::default(),
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            avatar_url: None,
            created_at: Utc::now(),
        }],
        vec![(company_id, "member@acme.com".to_string())],
    ));
    let channels = Arc::new(MockChannelPersistence {
        departments: Default::default(),
        channels: Mutex::new(vec![desk]),
    });
    let thread_use_cases = ThreadUseCases::for_test(
        Arc::new(InMemoryThreads::for_company(company_id)),
        channels.clone(),
        company_persistence.clone(),
        Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
        Arc::new(ThreadSettings {
            mail_domain: ("mailagents.com".to_string()).into(),
            max_spam_score: 5.0,
        }),
    );
    let mail = |message_id: &str, in_reply_to: Option<&str>, subject: &str| RawInboundPayload {
        headers: Some(match in_reply_to {
            Some(parent) => format!("Message-ID: {message_id}\nIn-Reply-To: {parent}\n"),
            None => format!("Message-ID: {message_id}\n"),
        }),
        to: "ops@acme.mailagents.com".to_string(),
        from: "member@acme.com".to_string(),
        subject: Some(subject.to_string()),
        text: Some("Body".to_string()),
        ..Default::default()
    };

    let opened = thread_use_cases
        .ingest_test_email(mail("<first@acme.com>", None, "Quarterly numbers"))
        .await
        .unwrap();
    assert!(opened.accepted, "a member writes to the members-only desk");

    // Leaving the department re-derives the desk's grants without them.
    channels.channels.lock().unwrap()[0]
        .principal_grants
        .clear();

    let reply = thread_use_cases
        .ingest_test_email(mail(
            "<second@acme.com>",
            Some("<first@acme.com>"),
            "Re: Quarterly numbers",
        ))
        .await
        .unwrap();
    assert!(
        reply.accepted,
        "a party to the thread may still reply in it"
    );
    let fresh = thread_use_cases
        .ingest_test_email(mail("<third@acme.com>", None, "Something new"))
        .await
        .unwrap();
    assert!(!fresh.accepted);
    assert_eq!(fresh.reason(), Some("Sender unauthorized for channel"));
}
