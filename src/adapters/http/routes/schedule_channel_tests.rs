use super::*;
use crate::{
    adapters::persistence::{PostgresPersistence, test_support::own_database},
    use_cases::{
        channel::{ChannelPersistence, ChannelWrite},
        company::{CompanyPersistence, CompanyWrite},
        user::UserPersistence,
    },
};

fn request(name: &str) -> ScheduleRequest {
    ScheduleRequest {
        name: name.into(),
        schedule_type: ScheduleType::Interval,
        interval_seconds: Some(3600),
        scheduled_at: None,
        subject_template: "Subject".into(),
        prompt_template: "Prompt".into(),
        delivery_mode: ScheduleDeliveryMode::MailboxOnly,
        recipient_emails: vec![],
        timezone: ScheduleTimezone::utc(),
        run_as_user_id: None,
        enabled: false,
    }
}

#[test]
fn schedule_json_body_cannot_request_a_channel_reassignment() {
    let mut body = serde_json::to_value(request("Rejected")).unwrap();
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
        use_cases,
    } = fixture(persistence).await;
    let created = create_channel_schedule_json(
        State(use_cases.clone()),
        AuthenticatedUser { id: user },
        Path((company, channels[1])),
        Json(request("Original")),
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
            Json(request("Rejected")),
        )
        .await;
        assert!(result.is_err());
        let after = use_cases
            .persistence()
            .get_by_id(created.id)
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
        Json(request("Edited")),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(edited.name, "Edited");
    assert_eq!(edited.channel_id, channels[1]);
    let reread = use_cases
        .persistence()
        .get_by_id(created.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reread.name, "Edited");
    assert_eq!(reread.channel_id, channels[1]);
}

struct Fixture {
    user: Uuid,
    company: Uuid,
    channels: Vec<Uuid>,
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
    let use_cases = Arc::new(ScheduleUseCases::new(
        persistence.clone(),
        persistence.clone(),
        persistence.clone(),
        persistence.clone(),
        persistence.clone(),
    ));
    Fixture {
        user,
        company,
        channels,
        use_cases,
    }
}
