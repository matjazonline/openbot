use super::*;
use crate::{
    adapters::{
        persistence::test_support::{OwnDatabase, own_database},
        workflow_source::WorkflowSourceDecoder,
    },
    application::{
        services::harness::{ClassificationRequest, TextClassifier},
        workflow::templates::TemplateCatalogue,
    },
    use_cases::{
        agent::{AgentPersistence, AgentUseCases, AgentWrite},
        channel::{ChannelPersistence, ChannelWrite},
        company::{CompanyModelConnectionWrite, CompanyPersistence, CompanyWrite},
        user::UserPersistence,
    },
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Notify, Semaphore};

struct Classifier {
    calls: AtomicUsize,
    active: AtomicUsize,
    pause: AtomicBool,
    fail: AtomicBool,
    entered: Notify,
    release: Semaphore,
}
impl Classifier {
    fn new(paused: bool) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            active: AtomicUsize::new(0),
            pause: AtomicBool::new(paused),
            fail: AtomicBool::new(false),
            entered: Notify::new(),
            release: Semaphore::new(0),
        }
    }
}
struct Active<'a>(&'a AtomicUsize);
impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
#[async_trait]
impl TextClassifier for Classifier {
    async fn complete(&self, request: ClassificationRequest<'_>) -> AppResult<String> {
        assert_eq!(request.api_key, "test-key");
        assert_eq!(request.provider.as_str(), "openai");
        assert_eq!(request.model.as_str(), "gpt-4o-mini");
        assert_eq!(request.user_prompt, "Help customers");
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.active.fetch_add(1, Ordering::SeqCst);
        let _active = Active(&self.active);
        self.entered.notify_one();
        if self.pause.load(Ordering::SeqCst) {
            self.release.acquire().await.unwrap().forget();
        }
        if self.fail.load(Ordering::SeqCst) {
            Err(AppError::Internal("injected provider failure".into()))
        } else {
            Ok("```You help customers.```".into())
        }
    }
}

struct Fixture {
    db: OwnDatabase,
    persistence: Arc<PostgresPersistence>,
    owner: Uuid,
    company: Uuid,
    classifier: Arc<Classifier>,
    service: OnboardingUseCases,
}
impl Fixture {
    async fn new(paused: bool) -> Self {
        let db = own_database()
            .await
            .expect("onboarding tests require PostgreSQL");
        let persistence = Arc::new(PostgresPersistence::new(db.pool.clone()));
        let owner = persistence
            .create_user("owner", "owner@example.test", "hash")
            .await
            .unwrap()
            .id;
        let company = CompanyPersistence::create(
            persistence.as_ref(),
            owner,
            CompanyWrite {
                name: "Onboarding".into(),
                slug: "onboarding".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        persistence
            .replace_model_connections_for_user(
                owner,
                company,
                vec![CompanyModelConnectionWrite {
                    provider: "openai".into(),
                    api_key: Some("test-key".into()),
                    models: vec!["gpt-4o-mini".into()],
                    is_default: true,
                }],
            )
            .await
            .unwrap();
        let classifier = Arc::new(Classifier::new(paused));
        let service = service(persistence.clone(), classifier.clone());
        Self {
            db,
            persistence,
            owner,
            company,
            classifier,
            service,
        }
    }
    fn submission(&self, key: Uuid, library: &[Uuid], custom: bool) -> OnboardingSubmission {
        OnboardingSubmission {
            submission_key: key,
            library_agent_ids: library.to_vec(),
            name: if custom {
                "Custom".into()
            } else {
                String::new()
            },
            instructions: if custom {
                "Help customers".into()
            } else {
                String::new()
            },
        }
    }
    async fn library(&self, slug: &str, name: &str) -> Uuid {
        self.persistence
            .create_library(AgentWrite {
                name: name.into(),
                slug: slug.into(),
                ..Default::default()
            })
            .await
            .unwrap()
            .id
    }
    async fn onboard(
        &self,
        key: Uuid,
        library: &[Uuid],
        custom: bool,
    ) -> AppResult<Vec<OnboardedAgent>> {
        completed(
            self.service
                .onboard(
                    self.owner,
                    self.company,
                    self.submission(key, library, custom),
                )
                .await?,
        )
    }
    async fn totals(&self) -> (i64, i64, i64) {
        sqlx::query_as(
            "SELECT (SELECT count(*) FROM agents WHERE company_id IS NOT NULL), \
            (SELECT count(*) FROM channels), (SELECT count(*) FROM workflow_provisioning_commands)",
        )
        .fetch_one(&self.db.pool)
        .await
        .unwrap()
    }
}
fn templates() -> ProvisioningDeployment {
    ProvisioningDeployment::new(
        Arc::new(TemplateCatalogue::shipped(&WorkflowSourceDecoder).unwrap()),
        &[],
        "agents.test",
    )
}
fn service(
    persistence: Arc<PostgresPersistence>,
    classifier: Arc<Classifier>,
) -> OnboardingUseCases {
    let agents = Arc::new(
        AgentUseCases::new(
            persistence.clone(),
            persistence.clone(),
            persistence.clone(),
        )
        .with_prompt_classifier(classifier),
    );
    OnboardingUseCases::new(
        persistence.clone(),
        persistence.clone(),
        Arc::new(PostgresOnboarding::new(
            persistence.as_ref().clone(),
            templates(),
        )),
        agents,
    )
}
fn completed(outcome: OnboardingOutcome) -> AppResult<Vec<OnboardedAgent>> {
    match outcome {
        OnboardingOutcome::Completed(agents) => Ok(agents),
        OnboardingOutcome::Rejected(message) => Err(AppError::BadRequest(message)),
    }
}
fn ids(results: &[OnboardedAgent]) -> Vec<(Uuid, Option<serde_json::Value>)> {
    results
        .iter()
        .map(|result| {
            (
                result.agent.id,
                result
                    .channel
                    .as_ref()
                    .map(|result| result.as_ref().expect("a channel was created").clone()),
            )
        })
        .collect()
}

#[tokio::test]
async fn onboarding_second_channel_error_rolls_back_every_effect() {
    let f = Fixture::new(false).await;
    let first = f.library("first", "First").await;
    let second = f.library("second", "Second").await;
    use crate::{
        entities::skill::SkillInstruction,
        use_cases::skill::{SkillManagementPersistence, SkillWrite},
    };
    let skill = SkillManagementPersistence::create_library(
        f.persistence.as_ref(),
        SkillWrite {
            slug: "support-skill".into(),
            name: "Support".into(),
            description: "Support rules".into(),
            trigger: "When helping customers".into(),
            instructions: vec![SkillInstruction::Prompt {
                text: "Be clear".into(),
            }],
            required_tools: None,
            created_by: None,
        },
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO agent_skills (agent_id, skill_id, position) VALUES ($1, $2, 0), ($3, $2, 0)",
    )
    .bind(first)
    .bind(skill.id)
    .bind(second)
    .execute(&f.db.pool)
    .await
    .unwrap();

    sqlx::raw_sql("CREATE FUNCTION fail_second_onboarding_channel() RETURNS trigger LANGUAGE plpgsql AS $$ \
        DECLARE observed bigint; BEGIN IF NEW.name = 'Second' THEN \
        SELECT count(*) INTO observed FROM agents WHERE company_id = NEW.company_id; \
        RAISE EXCEPTION 'onboarding-second-channel-after-%-agents', observed USING ERRCODE = 'P0001'; \
        END IF; RETURN NEW; END $$; CREATE TRIGGER fail_second BEFORE INSERT ON channels \
        FOR EACH ROW EXECUTE FUNCTION fail_second_onboarding_channel();")
        .execute(&f.db.pool).await.expect("fault installation must succeed");
    let error = f
        .onboard(Uuid::new_v4(), &[first, second], false)
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("onboarding-second-channel-after-2-agents"),
        "{error:?}"
    );
    assert!(matches!(error, AppError::Database(_)));
    assert_eq!(f.totals().await, (0, 0, 0));
    let skills: i64 =
        sqlx::query_scalar("SELECT count(*) FROM skills WHERE company_id IS NOT NULL")
            .fetch_one(&f.db.pool)
            .await
            .unwrap();
    assert_eq!(skills, 0);
}

#[tokio::test]
async fn onboarding_address_refusal_keeps_the_agent_and_other_channels() {
    let f = Fixture::new(false).await;
    ChannelPersistence::create(
        f.persistence.as_ref(),
        f.company,
        ChannelWrite {
            name: "Reserved".into(),
            slug: "taken".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let library = f.library("taken", "Taken").await;
    let created = f.onboard(Uuid::new_v4(), &[library], false).await.unwrap();
    assert!(matches!(
        &created[0].channel,
        Some(Err(ProvisioningRefusal::Conflict(_)))
    ));
    assert_eq!(f.totals().await, (1, 1, 0));
}

#[tokio::test]
async fn onboarding_same_key_reuses_results_and_new_key_after_deletion_is_fresh() {
    let f = Fixture::new(false).await;
    let library = f.library("desk", "Desk").await;
    let key = Uuid::new_v4();
    let first = f.onboard(key, &[library], false).await.unwrap();
    let again = f.onboard(key, &[library], false).await.unwrap();
    assert_eq!(ids(&first), ids(&again));
    assert_eq!(f.totals().await, (1, 1, 1));
    let channel = Uuid::parse_str(
        first[0].channel.as_ref().unwrap().as_ref().unwrap()["channel"]["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    // A channel with workflow configuration is intentionally retained. This private fixture has
    // no runs; remove its unused binding before exercising the supported channel/agent deletion.
    let removed =
        sqlx::query("DELETE FROM workflow_bindings WHERE company_id = $1 AND channel_id = $2")
            .bind(f.company)
            .bind(channel)
            .execute(&f.db.pool)
            .await
            .unwrap()
            .rows_affected();
    assert_eq!(removed, 1);
    ChannelPersistence::delete(f.persistence.as_ref(), channel)
        .await
        .unwrap();
    AgentPersistence::delete(f.persistence.as_ref(), first[0].agent.id)
        .await
        .unwrap();
    let fresh = f.onboard(Uuid::new_v4(), &[library], false).await.unwrap();
    assert_ne!(ids(&first), ids(&fresh));
    assert_eq!(f.totals().await, (1, 1, 2));
}

#[tokio::test]
async fn onboarding_new_key_after_rename_does_not_replay_an_old_channel() {
    let f = Fixture::new(false).await;
    let library = f.library("desk", "Desk").await;
    let first = f.onboard(Uuid::new_v4(), &[library], false).await.unwrap();
    AgentPersistence::update(
        f.persistence.as_ref(),
        first[0].agent.id,
        AgentWrite {
            name: "Archived desk".into(),
            slug: "archived".into(),
            provider: Some("openai".into()),
            model: Some("gpt-4o-mini".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let fresh = f.onboard(Uuid::new_v4(), &[library], false).await.unwrap();
    assert_ne!(fresh[0].agent.id, first[0].agent.id);
    assert!(matches!(
        fresh[0].channel,
        Some(Err(ProvisioningRefusal::Conflict(_)))
    ));
    assert_eq!(f.totals().await, (2, 1, 1));
}

#[tokio::test]
async fn onboarding_same_key_custom_competitors_wait_for_company_and_generate_once() {
    let f = Fixture::new(true).await;
    let key = Uuid::new_v4();
    let first_submission = f.submission(key, &[], true);
    let second_submission = f.submission(key, &[], true);
    let service = f.service.clone();
    let (owner, company) = (f.owner, f.company);
    let first =
        tokio::spawn(async move { service.onboard(owner, company, first_submission).await });
    tokio::time::timeout(Duration::from_secs(5), f.classifier.entered.notified())
        .await
        .unwrap();
    let service = f.service.clone();
    let second =
        tokio::spawn(async move { service.onboard(owner, company, second_submission).await });
    company_waiter(&f.db.pool).await;
    assert_eq!(f.classifier.calls.load(Ordering::SeqCst), 1);
    f.classifier.release.add_permits(1);
    let first = completed(
        tokio::time::timeout(Duration::from_secs(10), first)
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let second = completed(
        tokio::time::timeout(Duration::from_secs(10), second)
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(ids(&first), ids(&second));
    assert_eq!(f.classifier.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        first[0].agent.system_prompt.as_deref(),
        Some("You help customers.")
    );
    assert_eq!(f.totals().await, (1, 1, 1));
}

async fn company_waiter(pool: &sqlx::PgPool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity \
                WHERE datname = current_database() AND wait_event_type = 'Lock' \
                  AND query LIKE 'SELECT user_id FROM companies%FOR NO KEY UPDATE%')",
            )
            .fetch_one(pool)
            .await
            .unwrap();
            if waiting {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the competitor must demonstrably wait for the company lock");
}

#[tokio::test]
async fn onboarding_one_connection_progresses_with_real_custom_library_and_competitor() {
    let f = Fixture::new(true).await;
    let library = f.library("desk", "Desk").await;
    let one = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(f.db.pool.connect_options().as_ref().clone())
        .await
        .unwrap();
    let service = service(
        Arc::new(PostgresPersistence::new(one.clone())),
        f.classifier.clone(),
    );
    let key = Uuid::new_v4();
    let a = f.submission(key, &[library], true);
    let b = f.submission(key, &[library], true);
    let (owner, company) = (f.owner, f.company);
    let first_service = service.clone();
    let first = tokio::spawn(async move { first_service.onboard(owner, company, a).await });
    tokio::time::timeout(Duration::from_secs(5), f.classifier.entered.notified())
        .await
        .unwrap();
    assert_eq!(one.size(), 1);
    assert_eq!(one.num_idle(), 0);
    let started = Arc::new(Notify::new());
    let second_started = started.clone();
    let second = tokio::spawn(async move {
        second_started.notify_one();
        service.onboard(owner, company, b).await
    });
    started.notified().await;
    assert!(!first.is_finished());
    f.classifier.release.add_permits(1);
    let first = completed(
        tokio::time::timeout(Duration::from_secs(10), first)
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let second = completed(
        tokio::time::timeout(Duration::from_secs(10), second)
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(ids(&first), ids(&second));
    assert_eq!(f.classifier.calls.load(Ordering::SeqCst), 1);
    assert_eq!(f.totals().await, (2, 2, 2));
    one.close().await;
}

#[tokio::test]
async fn onboarding_provider_failure_releases_transaction_for_retry() {
    let f = Fixture::new(false).await;
    f.classifier.fail.store(true, Ordering::SeqCst);
    let key = Uuid::new_v4();
    let error = f.onboard(key, &[], true).await.unwrap_err();
    assert!(
        matches!(error, AppError::Internal(ref message) if message.contains("injected provider failure"))
    );
    assert_eq!(f.totals().await, (0, 0, 0));
    assert_eq!(f.classifier.active.load(Ordering::SeqCst), 0);
    f.classifier.fail.store(false, Ordering::SeqCst);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), f.onboard(key, &[], true))
            .await
            .unwrap()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn onboarding_cancellation_drops_provider_and_releases_company_lock() {
    let f = Fixture::new(true).await;
    let key = Uuid::new_v4();
    let submission = f.submission(key, &[], true);
    let service = f.service.clone();
    let (owner, company) = (f.owner, f.company);
    let task = tokio::spawn(async move { service.onboard(owner, company, submission).await });
    tokio::time::timeout(Duration::from_secs(5), f.classifier.entered.notified())
        .await
        .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(f.classifier.active.load(Ordering::SeqCst), 0);
    assert_eq!(f.totals().await, (0, 0, 0));
    f.classifier.pause.store(false, Ordering::SeqCst);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), f.onboard(key, &[], true))
            .await
            .unwrap()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn onboarding_denials_and_invalid_selection_have_no_provider_effects() {
    let f = Fixture::new(false).await;
    for role in ["admin", "member", "outsider"] {
        let user = f
            .persistence
            .create_user(role, &format!("{role}@example.test"), "hash")
            .await
            .unwrap()
            .id;
        if role != "outsider" {
            sqlx::query(
                "INSERT INTO company_members (company_id, user_id, role, id) VALUES ($1, $2, $3, $4)",
            )
            .bind(f.company)
            .bind(user)
            .bind(role)
            .bind(Uuid::new_v4())
            .execute(&f.db.pool)
            .await
            .unwrap();
        }
        let result = f
            .service
            .onboard(user, f.company, f.submission(Uuid::new_v4(), &[], true))
            .await;
        let begin = PostgresOnboarding::new(f.persistence.as_ref().clone(), templates())
            .begin(user, f.company)
            .await;
        if role == "outsider" {
            assert!(matches!(begin, Err(AppError::NotFound(_))));
        } else {
            assert!(matches!(begin, Err(AppError::Forbidden(_))));
        }
        if role == "outsider" {
            assert!(matches!(result, Err(AppError::NotFound(_))));
        } else {
            assert!(matches!(result, Err(AppError::Forbidden(_))));
        }
        assert!(matches!(
            f.service
                .onboard(
                    user,
                    Uuid::new_v4(),
                    f.submission(Uuid::new_v4(), &[], true)
                )
                .await,
            Err(AppError::NotFound(_))
        ));
    }
    for submission in [
        f.submission(Uuid::new_v4(), &[], false),
        f.submission(Uuid::new_v4(), &[Uuid::new_v4()], false),
    ] {
        assert!(matches!(
            f.service
                .onboard(f.owner, f.company, submission)
                .await
                .unwrap(),
            OnboardingOutcome::Rejected(_)
        ));
    }
    assert_eq!(f.classifier.calls.load(Ordering::SeqCst), 0);
    assert_eq!(f.totals().await, (0, 0, 0));
}

#[tokio::test]
async fn onboarding_prompt_and_operation_timeouts_cancel_and_allow_retry() {
    for prompt_timeout in [true, false] {
        let mut f = Fixture::new(true).await;
        let (operation, prompt) = if prompt_timeout {
            (Duration::from_secs(5), Duration::from_millis(500))
        } else {
            (Duration::from_millis(500), Duration::from_secs(5))
        };
        let short = f.service.clone().with_timeouts(operation, prompt);
        let key = Uuid::new_v4();
        let error = short
            .onboard(f.owner, f.company, f.submission(key, &[], true))
            .await
            .unwrap_err();
        assert!(
            matches!(error, AppError::ServiceUnavailable(ref message) if message.contains("timed out"))
        );
        use axum::response::IntoResponse;
        let response = error.into_response();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        );
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        assert!(
            String::from_utf8(body.to_vec())
                .unwrap()
                .contains("Try again")
        );
        assert_eq!(f.classifier.calls.load(Ordering::SeqCst), 1);
        assert_eq!(f.classifier.active.load(Ordering::SeqCst), 0);
        assert_eq!(f.totals().await, (0, 0, 0));
        f.classifier.pause.store(false, Ordering::SeqCst);
        f.service = f
            .service
            .with_timeouts(Duration::from_secs(5), Duration::from_secs(5));
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), f.onboard(key, &[], true))
                .await
                .unwrap()
                .unwrap()
                .len(),
            1
        );
    }
}

#[tokio::test]
async fn onboarding_library_model_lookup_failure_remains_an_infrastructure_error() {
    let f = Fixture::new(false).await;
    let library = AgentPersistence::create_library(
        f.persistence.as_ref(),
        AgentWrite {
            name: "Desk".into(),
            slug: "desk".into(),
            provider: Some("openai".into()),
            model: Some("gpt-4o-mini".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    sqlx::query("ALTER TABLE company_model_connections RENAME TO unavailable_model_connections")
        .execute(&f.db.pool)
        .await
        .expect("fault installation must succeed");
    let agents = AgentUseCases::new(
        f.persistence.clone(),
        f.persistence.clone(),
        f.persistence.clone(),
    );
    let error = agents
        .create_agent_from_library(
            f.owner,
            f.company,
            library,
            crate::use_cases::agent::LibraryCopyModel::FollowDefault,
        )
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::Database(_)), "{error:?}");
    let error = f
        .onboard(Uuid::new_v4(), &[library], false)
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::Database(_)), "{error:?}");
    assert_eq!(f.totals().await, (0, 0, 0));
}

#[tokio::test]
async fn onboarding_selection_is_bounded_and_custom_fields_are_paired() {
    let f = Fixture::new(false).await;
    let mut library = Vec::new();
    for index in 0..11 {
        library.push(
            f.library(&format!("desk-{index}"), &format!("Desk {index}"))
                .await,
        );
    }
    let too_many = f.submission(Uuid::new_v4(), &library, true);
    assert!(matches!(
        f.service
            .onboard(f.owner, f.company, too_many)
            .await
            .unwrap(),
        OnboardingOutcome::Rejected(_)
    ));
    for custom in [false, true] {
        let mut unpaired = f.submission(Uuid::new_v4(), &[], custom);
        unpaired.instructions = if custom {
            String::new()
        } else {
            "Help customers".into()
        };
        assert!(matches!(
            f.service
                .onboard(f.owner, f.company, unpaired)
                .await
                .unwrap(),
            OnboardingOutcome::Rejected(_)
        ));
    }
    assert_eq!(f.classifier.calls.load(Ordering::SeqCst), 0);
    assert_eq!(f.totals().await, (0, 0, 0));
    let boundary = f
        .onboard(Uuid::new_v4(), &library[..10], false)
        .await
        .unwrap();
    assert_eq!(boundary.len(), 10);
    assert_eq!(f.totals().await, (10, 10, 10));
}

#[tokio::test]
async fn onboarding_new_owner_with_new_key_does_not_replay_the_previous_owners_command() {
    use crate::{
        adapters::persistence::test_support::transfer_company_ownership,
        entities::company_member::CompanyAccessRole,
    };
    let f = Fixture::new(false).await;
    let library = f.library("desk", "Desk").await;
    let first = f.onboard(Uuid::new_v4(), &[library], false).await.unwrap();
    AgentPersistence::update(
        f.persistence.as_ref(),
        first[0].agent.id,
        AgentWrite {
            name: "Archive".into(),
            slug: "archive".into(),
            provider: Some("openai".into()),
            model: Some("gpt-4o-mini".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let owner = f
        .persistence
        .create_user("new-owner", "new-owner@example.test", "hash")
        .await
        .unwrap()
        .id;
    transfer_company_ownership(&f.db.pool, f.company, owner, CompanyAccessRole::Admin).await;
    let fresh = completed(
        f.service
            .onboard(
                owner,
                f.company,
                f.submission(Uuid::new_v4(), &[library], false),
            )
            .await
            .unwrap(),
    )
    .unwrap();
    assert_ne!(fresh[0].agent.id, first[0].agent.id);
    assert!(matches!(
        fresh[0].channel,
        Some(Err(ProvisioningRefusal::Conflict(_)))
    ));
    assert_eq!(f.totals().await, (2, 1, 1));
}
