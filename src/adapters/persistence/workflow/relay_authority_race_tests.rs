//! Competing production writers change authority after relay observation and before commit gates.
use super::*;
use crate::adapters::persistence::channel_gate::{
    ChannelGateAccess, ChannelGateKey, acquire_channel_gate_on,
};
use crate::application::agent::AgentPublicationScope;
use crate::application::ports::channel::ChannelUpdate;
use crate::entities::agent_capability::AgentId;

#[derive(Clone, Copy)]
enum Revoke {
    SourceAddress,
    TargetSettings,
}

async fn gate_waiter(p: &PostgresPersistence, blocker: i32, access: ChannelGateAccess) {
    let pattern = match access {
        ChannelGateAccess::Admit => "SELECT pg_advisory_xact_lock_shared(%",
        ChannelGateAccess::ChangeSettings => "SELECT pg_advisory_xact_lock(%",
    };
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)) AND query LIKE $2)",
            ).bind(blocker).bind(pattern).fetch_one(p.pool()).await.unwrap();
            if waiting { return; }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.expect("the actual writer/relay must reach its channel gate");
}

async fn pending_target_edit(f: &QueuedRelay, blocker: i32) -> tokio::task::JoinHandle<()> {
    let p = f.case.people.role.p();
    let desk = f.target.desk_id().unwrap();
    let old = ChannelPersistence::get_by_id(p, desk)
        .await
        .unwrap()
        .unwrap();
    let p = p.clone();
    let writer = tokio::spawn(async move {
        ChannelPersistence::update(
            &p,
            ChannelUpdate {
                actor: crate::adapters::persistence::test_support::company_owner_actor(
                    &p,
                    old.company_id,
                )
                .await,
                company_id: old.company_id,
                channel_id: desk,
                write: ChannelWrite {
                    name: old.name,
                    slug: old.slug.to_string(),
                    enabled: false,
                    ..Default::default()
                },
            },
        )
        .await
        .unwrap();
    });
    gate_waiter(
        f.case.people.role.p(),
        blocker,
        ChannelGateAccess::ChangeSettings,
    )
    .await;
    writer
}

async fn effects(f: &QueuedRelay) -> Value {
    let mut result = serde_json::Map::new();
    for table in [
        "messages",
        "threads",
        "thread_messages",
        "thread_principals",
        "participant_identities",
        "message_participants",
        "email_message_metadata",
        "external_messages",
        "external_threads",
        "workflow_message_intents",
        "message_internal_relays",
        "message_deliveries",
    ] {
        let rows: Value = sqlx::query_scalar(&format!(
            "SELECT COALESCE(jsonb_agg(to_jsonb(record) ORDER BY to_jsonb(record)::text),'[]'::jsonb) \
             FROM {table} AS record WHERE company_id=$1"
        )).bind(f.case.people.role.company().as_uuid()).fetch_one(f.case.people.role.p().pool()).await.unwrap();
        result.insert(table.into(), rows);
    }
    result.into()
}

async fn revoke_at_gate(f: &QueuedRelay, change: Revoke) {
    let p = f.case.people.role.p();
    let desk = f.target.desk_id().unwrap();
    let mut blocker = p.pool().begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    acquire_channel_gate_on(
        &mut blocker,
        ChannelGateKey::new(f.case.people.role.company().as_uuid(), desk),
        ChannelGateAccess::ChangeSettings,
    )
    .await
    .unwrap();
    let writer = match change {
        Revoke::TargetSettings => Some(pending_target_edit(f, pid).await),
        Revoke::SourceAddress => None,
    };
    let threads = relay_threads(p);
    let proof = InternalRelayProof::claiming(
        f.case.people.role.company().as_uuid(),
        f.delivery,
        PartIndex::new(0),
    );
    let mail = hop(&f.email, proof);
    let mutation = async {
        // This shared gate is inside commit_as_on, after real proof/ACL observation and before
        // any provider key, thread or canonical effect. PostgreSQL also queues the settings
        // writer ahead of the receiving claimant, so its commit must be observed on release.
        gate_waiter(p, pid, ChannelGateAccess::Admit).await;
        if matches!(change, Revoke::SourceAddress) {
            crate::adapters::persistence::test_support::rename_native_agent(
                p,
                AgentPublicationScope {
                    company: f.case.people.role.company(),
                    actor: f.case.people.role.admission.binding.target.actor,
                    agent: AgentId::new(f.case.created.agent.id),
                },
                "renamed-relay-sender",
            )
            .await;
        }
        blocker.commit().await.unwrap();
        if let Some(writer) = writer {
            writer.await.unwrap();
        }
    };
    let (outcome, ()) = tokio::join!(threads.relay_internal(&mail), mutation);
    match change {
        Revoke::SourceAddress => assert!(
            matches!(outcome, Ok(RelayDisposition::Refused(_))),
            "{outcome:?}"
        ),
        Revoke::TargetSettings => assert!(
            matches!(outcome, Err(AppError::Superseded(_))),
            "{outcome:?}"
        ),
    }
    assert!(
        matches!(
            threads.relay_internal(&mail).await.unwrap(),
            RelayDisposition::Refused(_)
        ),
        "retry/redelivery must reauthorize current source and target"
    );
}

async fn race(change: Revoke) {
    for redelivery in [false, true] {
        let f = Box::pin(queued(Receiver::SameDepartment)).await;
        if redelivery {
            let smtp = Box::pin(deliver(&f.case, relay_threads(f.case.people.role.p()))).await;
            assert!(smtp.sent().is_empty());
            assert_eq!(received(&f).await.len(), 1);
        }
        let before = effects(&f).await;
        Box::pin(revoke_at_gate(&f, change)).await;
        assert_eq!(
            effects(&f).await,
            before,
            "revocation must leave every receiving effect unchanged"
        );
    }
}

#[tokio::test]
async fn relay_identity_source_rename_after_observation_refuses_commit_and_redelivery() {
    Box::pin(race(Revoke::SourceAddress)).await;
}

#[tokio::test]
async fn relay_identity_target_revocation_after_observation_refuses_commit_and_redelivery() {
    Box::pin(race(Revoke::TargetSettings)).await;
}
