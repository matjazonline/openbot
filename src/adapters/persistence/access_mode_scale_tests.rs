//! A scale probe of the real root-membership writer, isolated from other test databases.
use super::*;
use crate::entities::company_member::CompanyAccessRole;
use crate::entities::value_objects::EmailAddress;
use crate::use_cases::company_invite::{
    CompanyInvitePersistence, CreateCompanyInvite, RemoveCompanyMember,
};
use std::time::Instant;

const MEMBERS: i64 = 1000;
const DESKS: usize = 200;

struct Scale {
    persistence: PostgresPersistence,
    owner: Uuid,
    company: Uuid,
    desks: Vec<Uuid>,
}

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
struct Writes {
    inserted: i64,
    removed: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct GrantStorage {
    rows: i64,
    table_bytes: i64,
    index_bytes: i64,
}

struct InviteCandidate {
    user: Uuid,
    email: EmailAddress,
    invite: Uuid,
}

impl Scale {
    async fn new(pool: PgPool, members: i64, desks: usize) -> Self {
        let persistence = PostgresPersistence::new(pool.clone());
        let owner = user(&pool, "scale-owner").await;
        let (company, _) = company(&pool, owner).await;
        let root: Uuid =
            sqlx::query_scalar("SELECT id FROM departments WHERE company_id=$1 AND is_root")
                .bind(company)
                .fetch_one(&pool)
                .await
                .unwrap();
        let member_started = Instant::now();
        for _ in 1..members {
            teammate(&pool, company, "member").await;
        }
        eprintln!(
            "access_mode_setup members={members} member_ms={}",
            member_started.elapsed().as_millis()
        );
        let desk_started = Instant::now();
        let mut created = Vec::new();
        for index in 0..desks {
            let desk = ChannelPersistence::create(
                &persistence,
                company,
                desk_write(&format!("scale-{index}"), None),
            )
            .await
            .unwrap();
            assert_eq!(
                desk.department_id.as_uuid(),
                root,
                "creation defaults to root"
            );
            created.push(desk.id);
            if (index + 1) % 50 == 0 {
                eprintln!(
                    "access_mode_setup desks={} desk_ms={}",
                    index + 1,
                    desk_started.elapsed().as_millis()
                );
            }
        }
        Self {
            persistence,
            owner,
            company,
            desks: created,
        }
    }

    async fn grants(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM channel_principal_grants WHERE company_id=$1 AND channel_id=ANY($2) AND provenance='department_membership'")
            .bind(self.company).bind(&self.desks).fetch_one(self.persistence.pool()).await.unwrap()
    }

    async fn grant_snapshot(&self) -> serde_json::Value {
        sqlx::query_scalar(
            "SELECT jsonb_agg(to_jsonb(permission) ORDER BY channel_id,principal_id,capability,provenance) \
             FROM channel_principal_grants AS permission WHERE company_id=$1",
        )
        .bind(self.company)
        .fetch_one(self.persistence.pool())
        .await
        .unwrap()
    }

    async fn revisions(&self) -> Vec<(Uuid, i64)> {
        sqlx::query_as("SELECT id,settings_revision FROM channels WHERE company_id=$1 AND id=ANY($2) ORDER BY id")
            .bind(self.company).bind(&self.desks).fetch_all(self.persistence.pool()).await.unwrap()
    }

    async fn writes(&self) -> Writes {
        sqlx::query_as("SELECT inserted,removed FROM access_mode_scale_writes")
            .fetch_one(self.persistence.pool())
            .await
            .unwrap()
    }

    async fn reset_writes(&self) {
        sqlx::query("UPDATE access_mode_scale_writes SET inserted=0,removed=0")
            .execute(self.persistence.pool())
            .await
            .unwrap();
    }

    async fn invitation(&self) -> InviteCandidate {
        let actor = user(self.persistence.pool(), "scale-candidate").await;
        // Invitation acceptance requires the account's current address to be verified.
        sqlx::query("UPDATE users SET email_verified_at=clock_timestamp() WHERE id=$1")
            .bind(actor)
            .execute(self.persistence.pool())
            .await
            .unwrap();
        let email: String = sqlx::query_scalar("SELECT email FROM users WHERE id=$1")
            .bind(actor)
            .fetch_one(self.persistence.pool())
            .await
            .unwrap();
        let invite = self
            .persistence
            .create_invite(CreateCompanyInvite {
                actor_id: self.owner,
                company_id: self.company,
                email: email.clone().into(),
                role: CompanyAccessRole::Member,
                access_level: AccessLevel::PUBLIC,
            })
            .await
            .unwrap();
        InviteCandidate {
            user: actor,
            email: email.into(),
            invite: invite.id,
        }
    }
}

async fn explain_grant_selection(f: &Scale) {
    let pool = f.persistence.pool();
    sqlx::query("ANALYZE channel_principal_grants")
        .execute(pool)
        .await
        .unwrap();
    let storage: GrantStorage = sqlx::query_as(
        "SELECT count(*) AS rows, \
                pg_relation_size('channel_principal_grants') AS table_bytes, \
                pg_indexes_size('channel_principal_grants') AS index_bytes \
           FROM channel_principal_grants",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    eprintln!(
        "access_mode_scale_storage rows={} table_bytes={} index_bytes={}",
        storage.rows, storage.table_bytes, storage.index_bytes
    );
    // This explains selection at the real cardinality; the timed commands below measure
    // production writes and their triggers, which a selection plan alone cannot establish.
    let plan: Vec<String> = sqlx::query_scalar(
        "EXPLAIN (ANALYZE, BUFFERS) \
         SELECT permission.principal_id FROM channel_principal_grants AS permission \
          WHERE permission.company_id=$1 AND permission.channel_id=ANY($2) \
            AND permission.provenance='department_membership'",
    )
    .bind(f.company)
    .bind(&f.desks)
    .fetch_all(pool)
    .await
    .unwrap();
    for line in plan {
        eprintln!("{line}");
    }
}

async fn install_counters(pool: &PgPool) {
    // Statement transition tables count real mutations without a per-row timing distortion.
    sqlx::raw_sql(
        r#"
        CREATE TABLE access_mode_scale_writes(inserted bigint NOT NULL, removed bigint NOT NULL);
        INSERT INTO access_mode_scale_writes VALUES(0,0);
        CREATE FUNCTION access_mode_scale_inserted() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN
            UPDATE access_mode_scale_writes SET inserted=inserted+(SELECT count(*) FROM added);
            RETURN NULL;
        END $$;
        CREATE TRIGGER access_mode_scale_inserted AFTER INSERT ON channel_principal_grants
            REFERENCING NEW TABLE AS added FOR EACH STATEMENT
            EXECUTE FUNCTION access_mode_scale_inserted();
        CREATE FUNCTION access_mode_scale_removed() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN
            UPDATE access_mode_scale_writes SET removed=removed+(SELECT count(*) FROM removed_rows);
            RETURN NULL;
        END $$;
        CREATE TRIGGER access_mode_scale_removed AFTER DELETE ON channel_principal_grants
            REFERENCING OLD TABLE AS removed_rows FOR EACH STATEMENT
            EXECUTE FUNCTION access_mode_scale_removed();
    "#,
    )
    .execute(pool)
    .await
    .unwrap();
}

fn assert_revision_step(before: &[(Uuid, i64)], after: &[(Uuid, i64)]) {
    assert!(!before.is_empty());
    assert_eq!(after.len(), before.len());
    for (before, after) in before.iter().zip(after) {
        assert_eq!(before.0, after.0);
        assert_eq!(after.1, before.1 + 1, "one revision step per affected desk");
    }
}

#[tokio::test]
#[ignore = "explicit 1000-member/200-desk scale measurement; run focused with --ignored"]
async fn access_mode_grants_scale_1000_members_200_desks() {
    let Some(database) = own_database().await else {
        return;
    };
    let setup = Instant::now();
    let f = Box::pin(Scale::new(database.pool.clone(), MEMBERS, DESKS)).await;
    let setup_elapsed = setup.elapsed();
    assert_eq!(f.desks.len(), DESKS);
    let expected = MEMBERS * DESKS as i64 * 2;
    assert_eq!(f.grants().await, expected);
    explain_grant_selection(&f).await;
    install_counters(f.persistence.pool()).await;
    let candidate = f.invitation().await;
    let before = f.revisions().await;
    let started = Instant::now();
    f.persistence
        .accept_pending_invite(candidate.invite, candidate.user, &candidate.email)
        .await
        .unwrap()
        .unwrap();
    let join_elapsed = started.elapsed();
    let join_writes = f.writes().await;
    assert_eq!(f.grants().await, expected + DESKS as i64 * 2);
    assert_eq!(
        join_writes,
        Writes {
            inserted: DESKS as i64 * 2,
            removed: 0
        }
    );
    let joined = f.revisions().await;
    assert_revision_step(&before, &joined);
    f.reset_writes().await;
    let started = Instant::now();
    f.persistence
        .remove_member(RemoveCompanyMember {
            actor_id: f.owner,
            company_id: f.company,
            member_user_id: candidate.user,
        })
        .await
        .unwrap();
    let removal_elapsed = started.elapsed();
    let removal_writes = f.writes().await;
    assert_eq!(f.grants().await, expected);
    assert_revision_step(&joined, &f.revisions().await);
    assert_eq!(
        removal_writes,
        Writes {
            inserted: 0,
            removed: DESKS as i64 * 2
        }
    );
    eprintln!(
        "access_mode_scale members={MEMBERS} desks={DESKS} grants={expected} setup_ms={} join_ms={} join_inserted={} join_removed={} removal_ms={} removal_inserted={} removal_removed={}",
        setup_elapsed.as_millis(),
        join_elapsed.as_millis(),
        join_writes.inserted,
        join_writes.removed,
        removal_elapsed.as_millis(),
        removal_writes.inserted,
        removal_writes.removed
    );
}

/// The ordinary CI run must catch a return to full rewrites without the explicit scale probe.
#[tokio::test]
async fn access_mode_grants_join_and_removal_write_only_the_delta() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = Box::pin(Scale::new(database.pool.clone(), 10, 3)).await;
    assert_eq!(f.grants().await, 60);
    let original_grants = f.grant_snapshot().await;
    install_counters(f.persistence.pool()).await;
    let candidate = f.invitation().await;
    let before = f.revisions().await;
    f.persistence
        .accept_pending_invite(candidate.invite, candidate.user, &candidate.email)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        f.writes().await,
        Writes {
            inserted: 6,
            removed: 0
        }
    );
    assert_eq!(f.grants().await, 66);
    let joined = f.revisions().await;
    assert_revision_step(&before, &joined);
    f.reset_writes().await;
    f.persistence
        .remove_member(RemoveCompanyMember {
            actor_id: f.owner,
            company_id: f.company,
            member_user_id: candidate.user,
        })
        .await
        .unwrap();
    assert_eq!(
        f.writes().await,
        Writes {
            inserted: 0,
            removed: 6
        }
    );
    assert_eq!(f.grants().await, 60);
    assert_eq!(f.grant_snapshot().await, original_grants);
    assert_revision_step(&joined, &f.revisions().await);
}

#[tokio::test]
async fn access_mode_grants_competing_root_join_and_default_desk_creation_keep_every_grant() {
    let Some(database) = own_database().await else {
        return;
    };
    let mut f = Box::pin(Scale::new(database.pool.clone(), 10, 3)).await;
    install_counters(f.persistence.pool()).await;
    let candidate = f.invitation().await;
    let before = f.revisions().await;
    let mut gate = f.persistence.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(f.company)
        .execute(&mut *gate)
        .await
        .unwrap();
    let (joined, created, ()) = tokio::join!(
        f.persistence
            .accept_pending_invite(candidate.invite, candidate.user, &candidate.email),
        ChannelPersistence::create(&f.persistence, f.company, desk_write("raced", None)),
        async {
            crate::adapters::persistence::test_support::wait_until_backends_are_blocked(
                f.persistence.pool(),
                2,
            )
            .await;
            gate.commit().await.unwrap();
        }
    );
    joined.unwrap().unwrap();
    let created = created.unwrap();
    assert_revision_step(&before, &f.revisions().await);
    f.desks.push(created.id);
    assert_eq!(
        f.grants().await,
        88,
        "all eleven root members have both grants on all four desks"
    );
    assert_eq!(
        f.writes().await,
        Writes {
            inserted: 28,
            removed: 0
        }
    );
    let candidate_grants: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM channel_principal_grants AS permission JOIN principals AS principal ON principal.company_id=permission.company_id AND principal.id=permission.principal_id WHERE permission.company_id=$1 AND permission.channel_id=ANY($2) AND principal.user_id=$3 AND permission.provenance='department_membership'")
        .bind(f.company).bind(&f.desks).bind(candidate.user).fetch_one(f.persistence.pool()).await.unwrap();
    assert_eq!(
        candidate_grants, 8,
        "neither serialization order loses the newly joined member"
    );
}
