use super::*;
use crate::application::agent::*;
use crate::application::local_agents::harness::NativeBundle;
use crate::application::local_agents::ports::LocalAgentRead;
use crate::entities::agent_capability::{AgentId, AgentRevisionId};
use crate::use_cases::agent::{AgentPersistence, AgentWrite};
use crate::use_cases::skill::{SkillManagementPersistence, SkillWrite};
use std::sync::Arc;

struct NativeSetup {
    s: Setup,
    scope: AgentPublicationScope,
    revision: AgentRevisionId,
    skill: Uuid,
}
impl NativeSetup {
    async fn new() -> Self {
        let mut s = Setup::new().await;
        let skill =
            s.f.persistence
                .create_company(
                    s.f.company,
                    SkillWrite {
                        slug: "native-skill".into(),
                        name: "Native".into(),
                        description: "Native supporting files".into(),
                        instructions: "Read scripts/run.sh and its adjacent data.".into(),
                        files: Some(vec![
                            crate::entities::skill_bundle::SkillFile::from_bytes(
                                "scripts/run.sh".into(),
                                b"#!/bin/sh\ncat data.bin\n",
                                true,
                            )
                            .unwrap(),
                            crate::entities::skill_bundle::SkillFile::from_bytes(
                                "scripts/data.bin".into(),
                                &[0, 255, 3],
                                false,
                            )
                            .unwrap(),
                        ]),
                        required_tools: None,
                        created_by: None,
                    },
                )
                .await
                .unwrap();
        let agent = AgentPersistence::create(
            s.f.persistence.as_ref(),
            s.f.company,
            AgentWrite {
                name: "Native only".into(),
                slug: "native-agent".into(),
                native_permissions: LocalPermissions::Auto,
                skill_ids: vec![skill.id],
                system_prompt: Some("Original native instructions".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let scope = AgentPublicationScope {
            company: CompanyId::new(s.f.company),
            actor: WorkflowActor::authenticated(s.f.owner).unwrap(),
            agent: AgentId::new(agent.id),
        };
        let revision =
            s.f.persistence
                .publish_agent(PublishAgent {
                    scope,
                    expected_draft_revision: agent.draft_revision,
                    expected_active_revision: None,
                    key: AgentPublicationKey::parse("native-initial").unwrap(),
                })
                .await
                .unwrap();
        s.profile.agent_id = Some(scope.agent);
        s.f.cases.save_profile(s.f.owner, &s.profile).await.unwrap();
        Self {
            s,
            scope,
            revision: revision.id,
            skill: skill.id,
        }
    }
    async fn connection(&self) -> ConnectionFence {
        self.s
            .f
            .persistence
            .acquire_connection(
                self.s.paired.candidate.credential.verifier(),
                self.s.paired.receipt.machine.installation_id,
                Uuid::new_v4(),
            )
            .await
            .unwrap()
            .unwrap()
    }
}

#[tokio::test]
async fn cloudroom_native_competing_starts_capture_one_revision_and_replay_before_profile_change() {
    let f = NativeSetup::new().await;
    let p = &f.s.f.persistence;
    let request = f.s.request();
    let (a, b) = tokio::join!(p.start_session(&request), p.start_session(&request));
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(a.operation.payload_digest(), b.operation.payload_digest());
    let LocalCommand::Create {
        selection, harness, ..
    } = a.operation.command()
    else {
        panic!("create")
    };
    assert_eq!(selection.policy.permissions, LocalPermissions::Auto);
    assert!(
        matches!(harness,NativeHarnessSelection::Agent { revision_id,.. } if *revision_id==f.revision)
    );
    let session = p
        .session(
            f.s.f.company,
            SessionId::new(request.request_id.as_uuid()).unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&session.harness, harness);
    let mut changed = f.s.profile.clone();
    changed.agent_id = None;
    changed.selection.policy.reasoning = LocalReasoningLevel::High;
    f.s.f
        .cases
        .save_profile(f.s.f.owner, &changed)
        .await
        .unwrap();
    let replay = p.start_session(&request).await.unwrap();
    assert_eq!(
        replay.operation.payload_digest(),
        a.operation.payload_digest()
    );
    let new = p.start_session(&f.s.request()).await.unwrap();
    assert!(matches!(
        new.operation.command(),
        LocalCommand::Create {
            harness: NativeHarnessSelection::Unconfigured,
            ..
        }
    ));
}

#[tokio::test]
async fn cloudroom_native_profile_uses_published_permissions_and_database_rejects_foreign_agent() {
    let f = NativeSetup::new().await;
    // A fallback full mode is unused when the published agent chooses auto.
    sqlx::query("UPDATE local_machine_capabilities SET catalog=jsonb_set(catalog,'{providers,0,permission_modes}','[\"auto\"]') WHERE company_id=$1")
        .bind(f.s.f.company).execute(&f.s.f.database.pool).await.unwrap();
    f.s.f
        .cases
        .save_profile(f.s.f.owner, &f.s.profile)
        .await
        .unwrap();
    let mut fallback = f.s.profile.clone();
    fallback.agent_id = None;
    assert!(
        f.s.f
            .cases
            .save_profile(f.s.f.owner, &fallback)
            .await
            .is_err()
    );
    let foreign = AgentPersistence::create(
        f.s.f.persistence.as_ref(),
        f.s.f.foreign,
        AgentWrite {
            name: "Foreign".into(),
            slug: "foreign".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let error =
        sqlx::query("UPDATE local_agent_profiles SET agent_id=$3 WHERE company_id=$1 AND id=$2")
            .bind(f.s.f.company)
            .bind(f.s.profile.id.as_uuid())
            .bind(foreign.id)
            .execute(&f.s.f.database.pool)
            .await
            .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23503")
    );
}

use crate::adapters::http::routes::local_machine_connection::{LocalMachineConnections, router};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;

struct NativeHttp {
    fixture: NativeSetup,
    operation: OperationId,
    fence: ConnectionFence,
    service: Arc<LocalMachineConnections>,
    app: axum::Router,
}
impl NativeHttp {
    async fn new() -> Self {
        let fixture = NativeSetup::new().await;
        let mut request = fixture.s.request();
        request.actor = fixture.s.f.admin;
        fixture
            .s
            .f
            .persistence
            .start_session(&request)
            .await
            .unwrap();
        let fence = fixture.connection().await;
        let service = Arc::new(LocalMachineConnections::new(
            fixture.s.f.persistence.clone(),
        ));
        let app = router::<Arc<LocalMachineConnections>>().with_state(service.clone());
        Self {
            fixture,
            operation: request.request_id,
            fence,
            service,
            app,
        }
    }
    fn token(&self) -> &str {
        self.fixture.s.paired.candidate.credential.expose_secret()
    }
    async fn get(
        &self,
        operation: OperationId,
        token: &str,
        fence: ConnectionFence,
        etag: Option<&str>,
    ) -> axum::response::Response {
        let mut request = Request::get(format!("/v1/local-machines/operations/{operation}/bundle"))
            .header("authorization", format!("Bearer {token}"))
            .header("x-openbot-protocol-version", "2")
            .header(
                "x-openbot-installation-id",
                fence.machine.installation_id.to_string(),
            )
            .header(
                "x-openbot-credential-generation",
                fence.machine.credential_generation.get(),
            )
            .header("x-openbot-connection-generation", fence.generation.get());
        if let Some(etag) = etag {
            request = request.header("if-none-match", etag);
        }
        self.app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap()
    }
    async fn etag(&self) -> String {
        let response = self
            .get(self.operation, self.token(), self.fence, None)
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        response.headers()["etag"].to_str().unwrap().to_owned()
    }
    async fn refused(&self, etag: &str) {
        for condition in [None, Some(etag)] {
            let response = self
                .get(self.operation, self.token(), self.fence, condition)
                .await;
            assert!(
                !response.status().is_success() && response.status() != StatusCode::NOT_MODIFIED
            );
            let bytes = axum::body::to_bytes(response.into_body(), 2 * 1024 * 1024)
                .await
                .unwrap();
            assert!(!bytes.windows(6).any(|part| part == b"scripts"));
        }
    }
}

#[tokio::test]
async fn cloudroom_native_bundle_http_authorizes_cached_reads_and_preserves_connection_identity() {
    let h = NativeHttp::new().await;
    let response = h.get(h.operation, h.token(), h.fence, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let etag = response.headers()["etag"].to_str().unwrap().to_owned();
    let bytes = axum::body::to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    let bundle: NativeBundle = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(bundle.instructions, "Original native instructions");
    assert_eq!(
        bundle.skills[0]
            .files
            .iter()
            .find(|file| file.path.as_str() == "scripts/data.bin")
            .unwrap()
            .bytes()
            .unwrap(),
        [0, 255, 3]
    );
    assert_eq!(
        format!("\"{}\"", NativeBundleDigest::of(&bytes).as_str()),
        etag
    );
    for (operation, token, status) in [
        (h.operation, h.token().to_owned(), StatusCode::NOT_MODIFIED),
        (
            OperationId::new(Uuid::new_v4()).unwrap(),
            h.token().to_owned(),
            StatusCode::NOT_FOUND,
        ),
        (h.operation, "0".repeat(64), StatusCode::UNAUTHORIZED),
    ] {
        let response = h.get(operation, &token, h.fence, Some(&etag)).await;
        assert_eq!(response.status(), status);
    }
    let mut wrong = h.fence;
    wrong.generation = ConnectionGeneration::try_from(h.fence.generation.get() + 1).unwrap();
    assert_eq!(
        h.get(h.operation, h.token(), wrong, Some(&etag))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let p = &h.fixture.s.f.persistence;
    let current:(i64,Uuid)=sqlx::query_as("SELECT connection_generation,connection_owner FROM local_machines WHERE company_id=$1 AND id=$2")
        .bind(h.fixture.s.f.company).bind(h.fence.machine.machine_id.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(current, (h.fence.generation.get() as i64, h.fence.owner_id));
    sqlx::query("UPDATE agent_skill_grants SET revoked_at=clock_timestamp() WHERE company_id=$1 AND skill_id=$2")
        .bind(h.fixture.s.f.company).bind(h.fixture.skill).execute(p.pool()).await.unwrap();
    h.refused(&etag).await;
    h.service.shutdown().await.unwrap();
}

#[tokio::test]
async fn cloudroom_native_bundle_refuses_a_valid_foreign_machine_and_revoked_original_actor() {
    let h = NativeHttp::new().await;
    let etag = h.etag().await;
    let f = &h.fixture.s.f;
    let foreign = f.pair(f.foreign).await;
    let fence = f
        .persistence
        .acquire_connection(
            foreign.candidate.credential.verifier(),
            foreign.receipt.machine.installation_id,
            Uuid::new_v4(),
        )
        .await
        .unwrap()
        .unwrap();
    let response = h
        .get(
            h.operation,
            foreign.candidate.credential.expose_secret(),
            fence,
            Some(&etag),
        )
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    sqlx::query("DELETE FROM company_members WHERE company_id=$1 AND user_id=$2")
        .bind(f.company)
        .bind(f.admin)
        .execute(&f.database.pool)
        .await
        .unwrap();
    h.refused(&etag).await;
    h.service.shutdown().await.unwrap();
}

#[tokio::test]
async fn cloudroom_native_capture_foreign_keys_preserve_history_and_allow_company_cascade() {
    let h = NativeHttp::new().await;
    let f = &h.fixture.s.f;
    let error = sqlx::query("DELETE FROM agent_revisions WHERE company_id=$1 AND id=$2")
        .bind(f.company)
        .bind(h.fixture.revision.as_uuid())
        .execute(&f.database.pool)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    let foreign=sqlx::query("INSERT INTO local_agent_sessions SELECT (jsonb_populate_record(NULL::local_agent_sessions,to_jsonb(source)||jsonb_build_object('id',$1::uuid,'agent_revision_id',$2::uuid))).* FROM local_agent_sessions AS source WHERE company_id=$3 LIMIT 1")
        .bind(Uuid::new_v4()).bind(Uuid::new_v4()).bind(f.company).execute(&f.database.pool).await.unwrap_err();
    assert_eq!(
        foreign.as_database_error().unwrap().code().as_deref(),
        Some("23503")
    );
    sqlx::query("DELETE FROM companies WHERE id=$1")
        .bind(f.company)
        .execute(&f.database.pool)
        .await
        .unwrap();
    let retained: i64 =
        sqlx::query_scalar("SELECT count(*) FROM local_agent_sessions WHERE company_id=$1")
            .bind(f.company)
            .fetch_one(&f.database.pool)
            .await
            .unwrap();
    assert_eq!(retained, 0);
    h.service.shutdown().await.unwrap();
}

#[tokio::test]
async fn cloudroom_native_competing_publication_and_admission_capture_a_coherent_revision() {
    let f = NativeSetup::new().await;
    let p = &f.s.f.persistence;
    let barrier = tokio::sync::Barrier::new(2);
    let request = f.s.request();
    let publishing = async {
        barrier.wait().await;
        let draft = AgentPersistence::update(
            p.as_ref(),
            f.scope.agent.as_uuid(),
            AgentWrite {
                name: "Edited".into(),
                slug: "native-agent".into(),
                system_prompt: Some("Changed native instructions".into()),
                native_permissions: LocalPermissions::Full,
                skill_ids: vec![f.skill],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        p.publish_agent(PublishAgent {
            scope: f.scope,
            expected_draft_revision: draft.draft_revision,
            expected_active_revision: Some(f.revision),
            key: AgentPublicationKey::parse("native-concurrent").unwrap(),
        })
        .await
        .unwrap()
    };
    let admitting = async {
        barrier.wait().await;
        p.start_session(&request).await.unwrap()
    };
    let (published, admitted) = tokio::join!(publishing, admitting);
    let LocalCommand::Create {
        harness, selection, ..
    } = admitted.operation.command()
    else {
        panic!("create")
    };
    let NativeHarnessSelection::Agent { revision_id, .. } = harness else {
        panic!("captured")
    };
    let mut tx = p.pool().begin().await.unwrap();
    let bundle =
        super::super::super::native::selected_on(&mut tx, f.s.f.company, f.s.f.owner, harness)
            .await
            .unwrap()
            .unwrap();
    if *revision_id == f.revision {
        assert_eq!(bundle.instructions, "Original native instructions");
        assert_eq!(selection.policy.permissions, LocalPermissions::Auto);
    } else {
        assert_eq!(*revision_id, published.id);
        assert_eq!(bundle.instructions, "Changed native instructions");
        assert_eq!(selection.policy.permissions, LocalPermissions::Full);
    }
    assert_eq!(bundle.permissions, selection.policy.permissions);
    tx.commit().await.unwrap();
}
