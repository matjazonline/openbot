use super::*;
use crate::domain::entities::{
    channel::ChannelAccessMode,
    creation::CreationProvenance,
    participant::{ChannelPrincipalGrant, GrantProvenance, PrincipalCapability},
    thread::ThreadParticipantProjection,
    transport::PrincipalId,
    value_objects::ChannelSlug,
};

fn channel(company_id: CompanyId, mode: ChannelAccessMode) -> Channel {
    Channel {
        id: Uuid::new_v4(),
        company_id: company_id.as_uuid(),
        name: "Related".into(),
        description: None,
        slug: ChannelSlug::from("related"),
        alias_slugs: vec![],
        participant_emails: None,
        access_mode: mode,
        principal_grants: vec![],
        enabled: true,
        add_3rd_party: false,
        created_by: CreationProvenance::system(),
        created_at: chrono::Utc::now(),
        settings_revision: 0,
    }
}

fn thread(channel_id: Uuid) -> Thread {
    Thread {
        id: Uuid::new_v4(),
        channel_id,
        subject: "Related".into(),
        participant_principal_ids: vec![],
        participant_projection: ThreadParticipantProjection::default(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

fn related(channel: &Channel) -> RelatedAssociation {
    RelatedAssociation::Channel(RelatedChannelId::new(channel.id))
}

fn admitted_request(
    company_id: CompanyId,
    version_id: VersionId,
    key: &str,
    association: RelatedAssociation,
) -> AdmitWorkflowRequest {
    let mut request = request(company_id, version_id, key, json!(1));
    request.association = association;
    request
}

fn set_membership(
    store: &MemoryStore,
    company_id: CompanyId,
    membership: CompanyMembership,
    principal_id: Option<PrincipalId>,
) {
    store.state.lock().unwrap().access.insert(
        (company_id, actor().user_id()),
        PrincipalAccessContext {
            principal_id,
            membership,
        },
    );
}

mod association;
mod cancellation;
mod membership;
mod reader_errors;
mod replay;
mod resources;
mod visibility;
