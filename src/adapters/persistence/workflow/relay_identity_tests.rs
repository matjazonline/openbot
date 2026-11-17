//! Real frozen agent replies pass through the delivery worker and canonical relay commit.
use super::*;
use crate::adapters::persistence::department::PostgresDepartments;
use crate::application::ports::channel::{ChannelPersistence, ChannelWrite};
use crate::application::ports::department::{
    DepartmentPersistence, DepartmentScope, DepartmentWrite,
};

enum Receiver {
    Root,
    SameDepartment,
    OtherDepartment,
    PrivateAgent,
}

struct QueuedRelay {
    case: AgentCase,
    delivery: DeliveryId,
    email: OutboundEmailV1,
    target: crate::entities::address::AddressTarget,
}

async fn queued(receiver: Receiver) -> QueuedRelay {
    let (f, _, _) = Box::pin(answering_case()).await;
    let target = receiving_target(&f, receiver).await;
    let address: String = sqlx::query_scalar(
        "SELECT address.slug || '@' || company.slug || '.agents.test' FROM addresses AS address \
         JOIN companies AS company ON company.id=address.company_id WHERE address.company_id=$1 \
         AND address.is_primary AND address.channel_id IS NOT DISTINCT FROM $2 \
         AND address.principal_id IS NOT DISTINCT FROM $3",
    )
    .bind(f.people.role.company().as_uuid())
    .bind(target.desk_id())
    .bind(target.principal_id().map(|id| id.as_uuid()))
    .fetch_one(f.people.role.p().pool())
    .await
    .unwrap();
    let (root, _) = Box::pin(settle_root(&f)).await;
    let (source, thread) = spoofed_from_department(&f, &address).await;
    let rerun = Box::pin(deliver_rerun(&f, root, thread)).await;
    let delivery = Box::pin(queued_reply(&f, rerun, source)).await;
    let email = frozen(&f, delivery).await;
    QueuedRelay {
        case: f,
        delivery,
        email,
        target,
    }
}

async fn receiving_target(
    f: &AgentCase,
    receiver: Receiver,
) -> crate::entities::address::AddressTarget {
    use crate::entities::{address::AddressTarget, transport::PrincipalId};
    let p = f.people.role.p();
    let department = match receiver {
        Receiver::PrivateAgent => {
            let agent = Box::pin(department_agent(f)).await;
            let principal: Uuid = sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND agent_id=$2 AND NOT open_to_public")
                .bind(f.people.role.company().as_uuid()).bind(agent).fetch_one(p.pool()).await.unwrap();
            return AddressTarget::Principal(PrincipalId::new(principal));
        }
        Receiver::Root => crate::entities::department::DepartmentId::new(
            sqlx::query_scalar("SELECT id FROM departments WHERE company_id=$1 AND is_root")
                .bind(f.people.role.company().as_uuid())
                .fetch_one(p.pool())
                .await
                .unwrap(),
        ),
        Receiver::SameDepartment => f.people.role.department,
        Receiver::OtherDepartment => {
            PostgresDepartments::new(
                p.clone(),
                crate::adapters::persistence::test_support::provisioning_templates(),
            )
            .create(
                DepartmentScope {
                    actor: f.people.role.admission.binding.target.actor,
                    company: f.people.role.company(),
                },
                DepartmentWrite {
                    name: "Unrelated department".into(),
                    slug: "unrelated".into(),
                    ..Default::default()
                },
                None,
            )
            .await
            .unwrap()
            .id
        }
    };
    let desk = ChannelPersistence::create(
        p,
        f.people.role.company().as_uuid(),
        ChannelWrite {
            name: "Restricted receiver".into(),
            slug: "restricted-relay".into(),
            enabled: true,
            department_id: Some(department),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    AddressTarget::Desk(desk.id)
}

#[derive(Debug, PartialEq, sqlx::FromRow)]
struct ReceivedHop {
    author_principal: Uuid,
    sender_principal: Uuid,
}

async fn received(f: &QueuedRelay) -> Vec<ReceivedHop> {
    sqlx::query_as(
        "SELECT message.author_principal_id AS author_principal,relay.sender_principal_id AS sender_principal \
         FROM message_internal_relays AS relay JOIN messages AS message \
           ON message.company_id=relay.company_id AND message.id=relay.message_id \
         JOIN thread_messages AS membership \
           ON membership.company_id=message.company_id AND membership.message_id=message.id \
         WHERE relay.company_id=$1 AND relay.delivery_id=$2 AND membership.channel_id IS NOT DISTINCT FROM $3 \
           AND membership.principal_id IS NOT DISTINCT FROM $4 \
           AND message.role='agent' AND message.direction='inbound'",
    )
    .bind(f.case.people.role.company().as_uuid())
    .bind(f.delivery.as_uuid())
    .bind(f.target.desk_id())
    .bind(f.target.principal_id().map(|id| id.as_uuid()))
    .fetch_all(f.case.people.role.p().pool())
    .await
    .unwrap()
}

#[tokio::test]
async fn relay_identity_department_member_uses_its_agent_grant_and_keeps_its_author() {
    let f = Box::pin(queued(Receiver::SameDepartment)).await;
    let smtp = Box::pin(deliver(&f.case, relay_threads(f.case.people.role.p()))).await;
    assert!(smtp.sent().is_empty());
    assert_eq!(delivered(&f.case, f.delivery).await, "delivered");
    let principal = f.case.created.principal_id.as_uuid();
    assert_eq!(
        received(&f).await,
        [ReceivedHop {
            author_principal: principal,
            sender_principal: principal
        }]
    );
}

#[tokio::test]
async fn relay_identity_nonmember_agent_is_refused_without_a_canonical_hop_or_smtp() {
    let f = Box::pin(queued(Receiver::OtherDepartment)).await;
    let smtp = Box::pin(deliver(&f.case, relay_threads(f.case.people.role.p()))).await;
    assert!(smtp.sent().is_empty());
    assert_eq!(delivered(&f.case, f.delivery).await, "dead_letter");
    assert!(received(&f).await.is_empty());
    let messages: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM thread_messages WHERE company_id=$1 AND channel_id=$2",
    )
    .bind(f.case.people.role.company().as_uuid())
    .bind(f.target.desk_id().unwrap())
    .fetch_one(f.case.people.role.p().pool())
    .await
    .unwrap();
    assert_eq!(messages, 0);
}

#[path = "relay_authority_race_tests.rs"]
mod authority_races;
#[path = "relay_private_agent_tests.rs"]
mod private_agent;

#[tokio::test]
async fn relay_identity_running_agent_cannot_send_as_another_agents_address() {
    let f = Box::pin(queued(Receiver::SameDepartment)).await;
    let other = Box::pin(department_agent(&f.case)).await;
    let principal: Uuid = sqlx::query_scalar("SELECT id FROM principals WHERE agent_id=$1")
        .bind(other)
        .fetch_one(f.case.people.role.p().pool())
        .await
        .unwrap();
    assert_ne!(principal, f.case.created.principal_id.as_uuid());
    let mut mail = f.email.clone();
    mail.from = mail
        .from
        .as_str()
        .replacen("escalation-handler@", "department-handler@", 1)
        .into();
    assert_ne!(mail.from, f.email.from);
    let proof = InternalRelayProof::claiming(
        f.case.people.role.company().as_uuid(),
        f.delivery,
        PartIndex::new(0),
    );
    let outcome = relay_threads(f.case.people.role.p())
        .relay_internal(&hop(&mail, proof))
        .await
        .unwrap();
    assert_eq!(
        outcome,
        RelayDisposition::Refused("Internal source proof does not match its delivery".into())
    );
    assert!(received(&f).await.is_empty());
    assert_eq!(delivered(&f.case, f.delivery).await, "pending");
}

#[tokio::test]
async fn relay_identity_root_members_accepts_a_real_agent_only_with_a_participate_grant() {
    let f = Box::pin(queued(Receiver::Root)).await;
    let company = f.case.people.role.company().as_uuid();
    let principal = f.case.created.principal_id.as_uuid();
    let target = f.target.desk_id().unwrap();
    let before: bool = sqlx::query_scalar("SELECT relay_desk_can_participate($1,$2,$3)")
        .bind(company)
        .bind(target)
        .bind(principal)
        .fetch_one(f.case.people.role.p().pool())
        .await
        .unwrap();
    assert!(!before, "this live agent has no root audience grant");
    sqlx::query("INSERT INTO channel_principal_grants(company_id,channel_id,principal_id,capability,provenance) VALUES($1,$2,$3,'participate','configured_allowlist')")
        .bind(company).bind(target).bind(principal).execute(f.case.people.role.p().pool()).await.unwrap();
    let smtp = Box::pin(deliver(&f.case, relay_threads(f.case.people.role.p()))).await;
    assert!(smtp.sent().is_empty());
    assert_eq!(delivered(&f.case, f.delivery).await, "delivered");
    assert_eq!(
        received(&f).await,
        [ReceivedHop {
            author_principal: principal,
            sender_principal: principal
        }]
    );
}

#[tokio::test]
async fn relay_identity_root_members_rejects_view_only_and_revoked_agent_grants_without_smtp() {
    let f = Box::pin(queued(Receiver::Root)).await;
    let company = f.case.people.role.company().as_uuid();
    let principal = f.case.created.principal_id.as_uuid();
    let target = f.target.desk_id().unwrap();
    for (capability, allowed) in [("view", false), ("participate", true)] {
        sqlx::query("INSERT INTO channel_principal_grants(company_id,channel_id,principal_id,capability,provenance) VALUES($1,$2,$3,$4,'configured_allowlist')")
            .bind(company).bind(target).bind(principal).bind(capability).execute(f.case.people.role.p().pool()).await.unwrap();
        let actual: bool = sqlx::query_scalar("SELECT relay_desk_can_participate($1,$2,$3)")
            .bind(company)
            .bind(target)
            .bind(principal)
            .fetch_one(f.case.people.role.p().pool())
            .await
            .unwrap();
        assert_eq!(
            actual, allowed,
            "{capability} alone determines this receiver capability"
        );
        sqlx::query("DELETE FROM channel_principal_grants WHERE company_id=$1 AND channel_id=$2 AND principal_id=$3 AND provenance='configured_allowlist'")
            .bind(company).bind(target).bind(principal).execute(f.case.people.role.p().pool()).await.unwrap();
    }
    let smtp = Box::pin(deliver(&f.case, relay_threads(f.case.people.role.p()))).await;
    assert!(smtp.sent().is_empty());
    assert_eq!(delivered(&f.case, f.delivery).await, "dead_letter");
    assert!(
        received(&f).await.is_empty(),
        "revocation prevents a canonical receiving hop"
    );
    let messages: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM thread_messages WHERE company_id=$1 AND channel_id=$2",
    )
    .bind(company)
    .bind(target)
    .fetch_one(f.case.people.role.p().pool())
    .await
    .unwrap();
    assert_eq!(messages, 0);
}
