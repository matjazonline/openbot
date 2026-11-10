use super::*;
use crate::application::ports::{
    channel::{ChannelPersistence, ChannelWrite},
    thread::ThreadPersistence,
};

fn form_request(path: &str, cookie: &HeaderValue, fields: &[(&str, String)]) -> Request<Body> {
    let mut result = request(Method::POST, path, cookie, json!({}));
    result.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-www-form-urlencoded"),
    );
    *result.body_mut() = Body::from(
        url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(fields.iter().map(|(name, value)| (*name, value.as_str())))
            .finish(),
    );
    result
}

async fn html(response: Response) -> String {
    String::from_utf8(
        to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

async fn profile_fixture() -> (Fixture, LocalAgentProfile) {
    let f = Fixture::new().await.unwrap();
    let paired = f.pair(f.company).await;
    f.install_catalog(&paired).await;
    let profile = f.profile(paired.receipt.machine.id);
    f.cases
        .set_projects(
            f.owner,
            f.company,
            paired.receipt.machine.id,
            std::slice::from_ref(&profile.selection.project_id),
        )
        .await
        .unwrap();
    f.cases.save_profile(f.owner, &profile).await.unwrap();
    (f, profile)
}

fn profile_fields(profile: &LocalAgentProfile) -> Vec<(&'static str, String)> {
    vec![
        ("name", "Edited <profile>".to_owned()),
        ("machine_id", profile.selection.machine_id.to_string()),
        ("project", profile.selection.project_id.to_string()),
        (
            "provider",
            serde_json::to_value(profile.selection.provider)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned(),
        ),
        ("model", profile.selection.model.as_str().to_owned()),
        (
            "reasoning",
            serde_json::to_value(profile.selection.policy.reasoning)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned(),
        ),
    ]
}

async fn readable_conversation(f: &Fixture) -> crate::domain::entities::thread::Thread {
    let channel = ChannelPersistence::create(
        f.persistence.as_ref(),
        f.company,
        ChannelWrite {
            name: "Paging conversation".into(),
            slug: format!("paging-{}", Uuid::new_v4().simple()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO threads(id,company_id,channel_id,subject) VALUES($1,$2,$3,'Paging conversation')")
        .bind(id).bind(f.company).bind(channel.id).execute(&f.database.pool).await.unwrap();
    f.persistence.get_thread_by_id(id).await.unwrap().unwrap()
}

#[tokio::test]
async fn cloudroom_management_ui_later_profile_page_and_selection_keep_authorized_start_conversation()
 {
    let (f, profile) = profile_fixture().await;
    sqlx::query("INSERT INTO local_agent_profiles(company_id,id,machine_id,name,selection) SELECT $1,gen_random_uuid(),$2,'Paging profile '||number::text,$3 FROM generate_series(1,101) AS number")
        .bind(f.company).bind(profile.selection.machine_id.as_uuid()).bind(serde_json::to_value(&profile.selection).unwrap()).execute(&f.database.pool).await.unwrap();
    let (target,name):(Uuid,String)=sqlx::query_as("SELECT id,name FROM local_agent_profiles WHERE company_id=$1 AND id<>$2 ORDER BY id DESC LIMIT 1")
        .bind(f.company).bind(profile.id.as_uuid()).fetch_one(&f.database.pool).await.unwrap();
    let thread = readable_conversation(&f).await;
    let (channel, conversation) = (thread.channel_id, thread.id);
    let (app, authority) = app(&f);
    let cookie = cookie(&f, &authority, f.owner).await;
    let base = format!("/ui/companies/{}/local-agents", f.company);
    let first = html(
        app.clone()
            .oneshot(request(
                Method::GET,
                &format!("{base}?channel={channel}&conversation={conversation}"),
                &cookie,
                json!({}),
            ))
            .await
            .unwrap(),
    )
    .await;
    let next = link(&first, "More profiles");
    let query = url::Url::parse(&format!("http://localhost{next}")).unwrap();
    assert!(
        query
            .query_pairs()
            .any(|(key, value)| key == "conversation" && value == conversation.to_string())
    );
    let second = html(
        app.clone()
            .oneshot(request(Method::GET, &next, &cookie, json!({})))
            .await
            .unwrap(),
    )
    .await;
    let selected = link(&second, &name);
    let selected = html(
        app.clone()
            .oneshot(request(Method::GET, &selected, &cookie, json!({})))
            .await
            .unwrap(),
    )
    .await;
    assert!(selected.contains(&format!("<option value=\"{target}\" selected>")));
    assert!(selected.contains(&format!(
        "name=\"conversation_id\" value=\"{conversation}\""
    )));
    let id = Uuid::now_v7();
    let fields = [
        ("request_id", id.to_string()),
        ("profile_id", target.to_string()),
        ("conversation_id", conversation.to_string()),
        ("prompt", "Use the later-page profile".to_owned()),
    ];
    let response = app
        .oneshot(form_request(&format!("{base}/sessions"), &cookie, &fields))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let saved: (Uuid, Uuid) =
        sqlx::query_as("SELECT profile_id,conversation_id FROM local_agent_sessions WHERE id=$1")
            .bind(id)
            .fetch_one(&f.database.pool)
            .await
            .unwrap();
    assert_eq!(saved, (target, conversation));
}

fn link(html: &str, label: &str) -> String {
    assert!(
        html.contains(&format!(">{label}</a>")),
        "Expected rendered navigation link {label}: {html}"
    );
    html.split(&format!(">{label}</a>"))
        .next()
        .unwrap()
        .rsplit("href=\"")
        .next()
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .replace("&amp;", "&")
}

#[tokio::test]
async fn cloudroom_management_ui_native_forms_parse_save_start_and_render_original_session() {
    let (f, profile) = profile_fixture().await;
    let (app, authority) = app(&f);
    let cookie = cookie(&f, &authority, f.owner).await;
    let base = format!("/ui/companies/{}/local-agents", f.company);
    let fields = profile_fields(&profile);
    let response = app
        .clone()
        .oneshot(form_request(
            &format!("{base}/profiles/{}", profile.id),
            &cookie,
            &fields,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let name: String =
        sqlx::query_scalar("SELECT name FROM local_agent_profiles WHERE company_id=$1 AND id=$2")
            .bind(f.company)
            .bind(profile.id.as_uuid())
            .fetch_one(&f.database.pool)
            .await
            .unwrap();
    assert_eq!(name, "Edited <profile>");
    let conversation = f.conversation(f.company).await;
    let id = Uuid::now_v7();
    let fields = [
        ("request_id", id.to_string()),
        ("profile_id", profile.id.to_string()),
        ("conversation_id", conversation.to_string()),
        ("prompt", "Native <task>".to_owned()),
    ];
    let response = app
        .clone()
        .oneshot(form_request(&format!("{base}/sessions"), &cookie, &fields))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        response.headers()[header::LOCATION],
        format!("{base}?session={id}")
    );
    let response = app
        .clone()
        .oneshot(request(
            Method::GET,
            &format!("{base}?session={id}"),
            &cookie,
            json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let rendered = html(response).await;
    assert!(
        rendered.contains("Native &lt;task&gt;")
            && rendered.contains("Measured usage: Unavailable")
    );
    assert!(!rendered.contains("Native <task>"));
    sqlx::query("UPDATE local_agent_sessions SET thread_id='thread_form_test' WHERE company_id=$1 AND id=$2")
        .bind(f.company).bind(id).execute(&f.database.pool).await.unwrap();
    let fields = [
        ("request_id", Uuid::now_v7().to_string()),
        ("kind", "inspect".to_owned()),
    ];
    let response = app
        .oneshot(form_request(
            &format!("{base}/sessions/{id}/commands"),
            &cookie,
            &fields,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM local_agent_operations")
        .fetch_one(&f.database.pool)
        .await
        .unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn cloudroom_management_ui_foreign_selection_and_form_mutations_cannot_cross_company() {
    let (f, profile) = profile_fixture().await;
    let pairing = f
        .cases
        .create_pairing(f.owner, f.company, None)
        .await
        .unwrap();
    let (app, authority) = app(&f);
    let cookie = cookie(&f, &authority, f.owner).await;
    let base = format!("/ui/companies/{}/local-agents", f.foreign);
    for query in [
        format!("machine={}", profile.selection.machine_id),
        format!("profile={}", profile.id),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(request(
                    Method::GET,
                    &format!("{base}?{query}"),
                    &cookie,
                    json!({})
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
    }
    let mutations = [
        (
            format!("{base}/pairings"),
            vec![("replace", profile.selection.machine_id.to_string())],
        ),
        (
            format!("{base}/pairings/{}/cancel", pairing.attempt.id),
            vec![],
        ),
        (
            format!("{base}/machines/{}/state", profile.selection.machine_id),
            vec![("change", "disable".into())],
        ),
        (
            format!("{base}/machines/{}/projects", profile.selection.machine_id),
            vec![("project", profile.selection.project_id.to_string())],
        ),
        (
            format!("{base}/machines/{}/test", profile.selection.machine_id),
            vec![("request_id", Uuid::now_v7().to_string())],
        ),
        (format!("{base}/profiles/{}/delete", profile.id), vec![]),
    ];
    for (path, fields) in mutations {
        let response = app
            .clone()
            .oneshot(form_request(&path, &cookie, &fields))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
}
