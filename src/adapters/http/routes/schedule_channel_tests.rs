use super::*;
use crate::domain::workflow::CompanyId;
use crate::{
    adapters::persistence::{
        PostgresPersistence,
        test_support::own_database,
        workflow::schedule_test_support::{schedulable_binding, schedule_use_cases},
    },
    use_cases::{
        channel::{ChannelPersistence, ChannelWrite},
        company::{CompanyPersistence, CompanyWrite},
        user::UserPersistence,
    },
};

fn request(name: &str, binding_id: Uuid) -> ScheduleRequest {
    ScheduleRequest {
        name: name.into(),
        schedule_type: ScheduleType::Interval,
        interval_seconds: Some(3600),
        scheduled_at: None,
        timezone: ScheduleTimezone::utc(),
        binding_id,
        enabled: false,
    }
}

#[test]
fn schedule_json_body_cannot_request_a_channel_reassignment() {
    let mut body = serde_json::to_value(request("Rejected", Uuid::new_v4())).unwrap();
    body["channel_id"] = serde_json::json!(Uuid::new_v4());
    assert!(serde_json::from_value::<ScheduleRequest>(body).is_err());
}

#[tokio::test]
async fn schedule_api_creation_selects_channel_and_update_cannot_reassign_it() {
    let Some(database) = own_database().await else {
        return;
    };
    let persistence = Arc::new(PostgresPersistence::new(database.pool.clone()));
    let Fixture {
        user,
        company,
        channels,
        binding,
        use_cases,
    } = fixture(persistence).await;
    let created = create_channel_schedule_json(
        State(use_cases.clone()),
        AuthenticatedUser { id: user },
        Path((company, channels[1])),
        Json(request("Original", binding)),
    )
    .await
    .unwrap()
    .1
    .0;
    assert_eq!(created.channel_id, channels[1]);
    for company_scope in [company, Uuid::new_v4()] {
        let result = update_schedule_json(
            State(use_cases.clone()),
            AuthenticatedUser { id: user },
            Path((company_scope, channels[0], created.id)),
            Json(request("Rejected", binding)),
        )
        .await;
        assert!(result.is_err());
        let after = use_cases
            .persistence()
            .get(CompanyId::new(company), created.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::to_value(&created).unwrap(),
            serde_json::to_value(after).unwrap()
        );
    }
    let edited = update_schedule_json(
        State(use_cases.clone()),
        AuthenticatedUser { id: user },
        Path((company, channels[1], created.id)),
        Json(request("Edited", binding)),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(edited.name, "Edited");
    assert_eq!(edited.channel_id, channels[1]);
    let reread = use_cases
        .persistence()
        .get(CompanyId::new(company), created.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reread.name, "Edited");
    assert_eq!(reread.channel_id, channels[1]);
}

/// F031: every route nested under a channel acts only on a schedule of that channel and that
/// company; another channel's or another company's schedule is not found, and nothing changes.
#[tokio::test]
async fn nested_schedule_routes_refuse_another_channels_or_companys_schedule() {
    let Some(database) = own_database().await else {
        return;
    };
    let persistence = Arc::new(PostgresPersistence::new(database.pool.clone()));
    let Fixture {
        user,
        company,
        channels,
        binding,
        use_cases,
    } = fixture(persistence.clone()).await;
    let schedule = create_channel_schedule_json(
        State(use_cases.clone()),
        AuthenticatedUser { id: user },
        Path((company, channels[1])),
        Json(request("Scoped", binding)),
    )
    .await
    .unwrap()
    .1
    .0;
    // The same owner's second company, with a channel of its own.
    let suffix = Uuid::new_v4().simple().to_string();
    let other_company = CompanyPersistence::create(
        persistence.as_ref(),
        user,
        CompanyWrite {
            name: "Other".into(),
            slug: suffix,
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let other_channel = ChannelPersistence::create(
        persistence.as_ref(),
        other_company,
        ChannelWrite {
            name: "other".into(),
            slug: "other".into(),
            enabled: false,
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let caller = || AuthenticatedUser { id: user };
    let not_found =
        |result: AppResult<()>| assert!(matches!(result, Err(AppError::NotFound(_))), "{result:?}");
    for (scope_company, scope_channel) in [
        (company, channels[0]),
        (other_company, other_channel),
        (other_company, channels[1]),
    ] {
        let path = || Path((scope_company, scope_channel, schedule.id));
        let uc = || State(use_cases.clone());
        not_found(get_schedule_json(uc(), caller(), path()).await.map(drop));
        not_found(
            list_schedule_runs_json(
                uc(),
                caller(),
                path(),
                axum::extract::Query(RunsQuery {
                    offset: 0,
                    limit: 10,
                }),
            )
            .await
            .map(drop),
        );
        not_found(
            run_schedule_now_json(
                uc(),
                caller(),
                path(),
                Json(RunNowBody {
                    command_key: "elsewhere".into(),
                }),
            )
            .await
            .map(drop),
        );
        not_found(
            toggle_schedule_json(uc(), caller(), path(), Json(ToggleBody { enabled: true }))
                .await
                .map(drop),
        );
        not_found(delete_schedule_json(uc(), caller(), path()).await.map(drop));
        not_found(
            ui_run_schedule_now(
                uc(),
                caller(),
                Path((scope_channel, schedule.id)),
                Form(UiRunNowForm {
                    company_id: scope_company,
                    command_key: None,
                }),
            )
            .await
            .map(drop),
        );
        not_found(
            ui_toggle_schedule(
                uc(),
                caller(),
                Path((scope_channel, schedule.id)),
                Form(UiToggleForm {
                    company_id: scope_company,
                    enabled: Some("true".into()),
                }),
            )
            .await
            .map(drop),
        );
        not_found(
            ui_delete_schedule(
                uc(),
                caller(),
                Path((scope_channel, schedule.id)),
                Form(UiScheduleActionQuery {
                    company_id: scope_company,
                }),
            )
            .await
            .map(drop),
        );
    }
    // Nothing happened to the schedule: it is still there, still paused, never run.
    let after = use_cases
        .persistence()
        .get(CompanyId::new(company), schedule.id)
        .await
        .unwrap()
        .unwrap();
    assert!(!after.enabled);
    assert!(
        use_cases
            .persistence()
            .list_runs(CompanyId::new(company), schedule.id, 0, 10)
            .await
            .unwrap()
            .is_empty()
    );
    // The schedule's own path works.
    let runs = list_schedule_runs_json(
        State(use_cases.clone()),
        caller(),
        Path((company, channels[1], schedule.id)),
        axum::extract::Query(RunsQuery {
            offset: 0,
            limit: 10,
        }),
    )
    .await
    .unwrap()
    .0;
    assert!(runs.is_empty());
    // Bindings are offered only for a channel of the path's company.
    not_found(
        list_schedule_bindings_json(
            State(use_cases.clone()),
            caller(),
            Path((company, other_channel)),
        )
        .await
        .map(drop),
    );
    let offered = list_schedule_bindings_json(
        State(use_cases.clone()),
        caller(),
        Path((company, channels[1])),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(
        offered
            .iter()
            .map(|choice| choice.binding_id)
            .collect::<Vec<_>>(),
        vec![binding]
    );
}

struct Fixture {
    user: Uuid,
    company: Uuid,
    channels: Vec<Uuid>,
    binding: Uuid,
    use_cases: Arc<ScheduleUseCases>,
}

async fn fixture(persistence: Arc<PostgresPersistence>) -> Fixture {
    let suffix = Uuid::new_v4().simple().to_string();
    let user = persistence
        .create_user(&suffix, &format!("{suffix}@example.test"), "hash")
        .await
        .unwrap()
        .id;
    let company = CompanyPersistence::create(
        persistence.as_ref(),
        user,
        CompanyWrite {
            name: "Schedule API".into(),
            slug: suffix,
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let mut channels = Vec::new();
    for slug in ["first", "selected"] {
        channels.push(
            ChannelPersistence::create(
                persistence.as_ref(),
                company,
                ChannelWrite {
                    name: slug.into(),
                    slug: slug.into(),
                    enabled: false,
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .id,
        );
    }
    let binding = schedulable_binding(persistence.as_ref(), company, user).await;
    Fixture {
        user,
        company,
        channels,
        binding,
        use_cases: Arc::new(schedule_use_cases(persistence.as_ref())),
    }
}
