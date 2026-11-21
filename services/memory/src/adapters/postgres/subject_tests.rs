use super::storage_tests::{self as support, Fixture, exact, saved_id};
use crate::application::{operations::Surface, service::Principal};
use axum::{Router, http::StatusCode};
use serde_json::{Value, json};
use uuid::Uuid;

const LIST: &str = "/admin/v1/observations/subject/list";
const DELETE: &str = "/admin/v1/observations/subject/delete";

struct SubjectFixture {
    f: Fixture,
    router: Router,
    admin: String,
    actor: Principal,
    client: String,
    client_actor: Principal,
}

impl SubjectFixture {
    async fn new() -> Self {
        let f = support::fixture().await;
        let issued = f
            .run(
                &f.operator,
                "admin.admin_credentials.issue",
                json!({"scope":"provisioner","label":"subject cleanup","expires_at":null}),
            )
            .await
            .unwrap();
        let admin = issued["token"].as_str().unwrap().to_owned();
        let actor = f
            .app
            .authenticate(Some(&admin), Surface::Admin, chrono::Utc::now())
            .await
            .unwrap();
        f.run(
            &actor,
            "admin.tenants.create",
            json!({"external_id":"subject-owner"}),
        )
        .await
        .unwrap();
        for namespace in ["one", "two", "private"] {
            f.run(
                &actor,
                "admin.namespaces.create",
                json!({"tenant":"subject-owner","namespace":namespace}),
            )
            .await
            .unwrap();
        }
        let (client, client_actor) = f
            .client(
                "subject-owner",
                &[("one", 9, 9), ("two", 9, 9), ("private", 9, 9)],
            )
            .await;
        let router = support::pg_router(&f);
        Self {
            f,
            router,
            admin,
            actor,
            client,
            client_actor,
        }
    }

    async fn save(&self, namespace: &str, subject: &str, key: &str, count: usize) -> Vec<Uuid> {
        let mut body = support::save(key, namespace, support::context(json!(9), exact(4)), count);
        for item in body["items"].as_array_mut().unwrap() {
            item["subject"] = json!(subject);
        }
        let (status, answer) =
            support::http(&self.router, "/v1/observations/save", &self.client, body).await;
        assert_eq!(status, StatusCode::OK, "{answer}");
        answer["observations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| Uuid::parse_str(item["observation_id"].as_str().unwrap()).unwrap())
            .collect()
    }

    fn selector(namespace: &str, subject: &str) -> Value {
        json!({"tenant":"subject-owner","namespace":namespace,"subject":subject})
    }

    async fn list(&self, namespace: &str, subject: &str) -> Value {
        let (status, answer) = support::http(
            &self.router,
            LIST,
            &self.admin,
            Self::selector(namespace, subject),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{answer}");
        answer
    }

    async fn delete(&self, namespace: &str, subject: &str, ids: &[Uuid]) -> Value {
        let mut body = Self::selector(namespace, subject);
        body["observation_ids"] = json!(ids);
        let (status, answer) = support::http(&self.router, DELETE, &self.admin, body).await;
        assert_eq!(status, StatusCode::OK, "{answer}");
        answer
    }
}

#[tokio::test]
async fn memory_subject_http_preserves_provenance_corrections_and_exact_disabled_expired_cleanup() {
    let f = SubjectFixture::new().await;
    let one = f.save("one", "person-a", "subject-one", 2).await;
    let two = f.save("two", "person-a", "subject-two", 1).await;
    let unrelated = f.save("one", "person-b", "other-subject", 1).await;
    let private = f.save("private", "person-a", "principal-memory", 1).await;
    f.f.run(
        &f.client_actor,
        "observations.correct",
        support::correct(
            "correct-subject",
            "one",
            one[0],
            1,
            9,
            support::context(json!(9), exact(4)),
        ),
    )
    .await
    .unwrap();
    let subject: Option<String> =
        sqlx::query_scalar("SELECT subject FROM observations WHERE id=$1")
            .bind(one[0])
            .fetch_one(f.f.db.pool())
            .await
            .unwrap();
    assert_eq!(subject.as_deref(), Some("person-a"));
    sqlx::query(
        "UPDATE observations SET expires_at=clock_timestamp()-interval '1 hour' WHERE id=$1",
    )
    .bind(one[1])
    .execute(f.f.db.pool())
    .await
    .unwrap();
    f.f.run(
        &f.actor,
        "admin.tenants.disable",
        json!({"tenant":"subject-owner"}),
    )
    .await
    .unwrap();
    let page = f.list("one", "person-a").await;
    assert_eq!(page["items"].as_array().unwrap().len(), 2);
    assert!(
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["access_level"] == 4
                && item.get("body").is_none()
                && item.get("title").is_none())
    );
    let mut mixed = one.clone();
    mixed.extend(unrelated.iter());
    mixed.extend(two.iter());
    let deleted = f.delete("one", "person-a", &mixed).await;
    assert_eq!(
        deleted["deleted"].as_array().unwrap().len(),
        2,
        "foreign namespace/subject IDs are rechecked"
    );
    assert!(
        f.delete("one", "person-a", &one).await["deleted"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        f.list("two", "person-a").await["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        f.list("one", "person-b").await["items"][0]["observation_id"],
        json!(unrelated[0])
    );
    assert_eq!(
        f.list("private", "person-a").await["items"][0]["observation_id"],
        json!(private[0])
    );
    f.delete("two", "person-a", &two).await;
    assert!(
        f.list("two", "person-a").await["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn memory_subject_http_pages_bounds_foreign_management_and_client_authentication() {
    let f = SubjectFixture::new().await;
    let mut ids = f.save("one", "person-a", "page-one", 64).await;
    ids.extend(f.save("one", "person-a", "page-two", 37).await);
    let first = f.list("one", "person-a").await;
    assert_eq!(first["items"].as_array().unwrap().len(), 100);
    assert_eq!(first["next_after"], first["items"][99]["observation_id"]);
    let mut next = SubjectFixture::selector("one", "person-a");
    next["after"] = first["next_after"].clone();
    let (status, second) = support::http(&f.router, LIST, &f.admin, next).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(second["items"].as_array().unwrap().len(), 1);
    assert!(second["next_after"].is_null());
    for limit in [0, 101] {
        let mut body = SubjectFixture::selector("one", "person-a");
        body["limit"] = json!(limit);
        assert_eq!(
            support::http(&f.router, LIST, &f.admin, body).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    for body in [json!([]), json!(ids), json!([ids[0], ids[0]])] {
        let mut request = SubjectFixture::selector("one", "person-a");
        request["observation_ids"] = body;
        assert_eq!(
            support::http(&f.router, DELETE, &f.admin, request).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    f.f.tenant("foreign", &["one"]).await;
    let mut foreign = SubjectFixture::selector("one", "person-a");
    foreign["tenant"] = json!("foreign");
    assert_eq!(
        support::http(&f.router, LIST, &f.admin, foreign).await.0,
        StatusCode::NOT_FOUND
    );
    // A client credential refuses before this deliberately incomplete body is parsed.
    assert_eq!(
        support::http(&f.router, LIST, &f.client, json!({})).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn memory_subject_competing_purgers_and_save_serialize_without_resurrection() {
    let f = SubjectFixture::new().await;
    let old = f.save("one", "person-a", "before-purge", 1).await;
    // Pause a real save after it acquired its namespace SHARE lock. The purger
    // must wait for that production lock, independently of queue fairness.
    sqlx::raw_sql("CREATE FUNCTION subject_save_barrier() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
        PERFORM pg_advisory_xact_lock(73434); RETURN NEW; END $$;
        CREATE TRIGGER subject_save_barrier BEFORE INSERT ON observations FOR EACH ROW EXECUTE FUNCTION subject_save_barrier()")
        .execute(f.f.db.pool()).await.unwrap();
    let mut barrier = f.f.db.pool().begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(73434)")
        .execute(&mut *barrier)
        .await
        .unwrap();
    let router = f.router.clone();
    let token = f.client.clone();
    let mut payload = support::save(
        "after-purge",
        "one",
        support::context(json!(9), exact(4)),
        1,
    );
    payload["items"][0]["subject"] = json!("person-a");
    let saving = tokio::spawn(async move {
        support::http(&router, "/v1/observations/save", &token, payload).await
    });
    wait_for_locks(&f, "%INSERT INTO observations%", 1).await;
    let mut request = SubjectFixture::selector("one", "person-a");
    request["observation_ids"] = json!(old);
    let router = f.router.clone();
    let token = f.admin.clone();
    let deleting =
        tokio::spawn(async move { support::http(&router, DELETE, &token, request).await });
    wait_for_locks(&f, "%namespaces%", 1).await;
    assert!(!saving.is_finished());
    assert!(!deleting.is_finished());
    barrier.rollback().await.unwrap();
    let deleted = deleting.await.unwrap();
    assert_eq!(deleted.0, StatusCode::OK);
    let saved = saving.await.unwrap();
    assert_eq!(saved.0, StatusCode::OK);
    let new = [saved_id(&saved.1)];
    assert_eq!(deleted.1["deleted"], json!(old));
    let mut barrier = f.f.db.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM namespaces WHERE external_id='one' FOR SHARE")
        .execute(&mut *barrier)
        .await
        .unwrap();
    let spawn_purge = || {
        let router = f.router.clone();
        let token = f.admin.clone();
        let mut request = SubjectFixture::selector("one", "person-a");
        request["observation_ids"] = json!(new);
        tokio::spawn(async move { support::http(&router, DELETE, &token, request).await })
    };
    let left = spawn_purge();
    let right = spawn_purge();
    wait_for_locks(&f, "%namespaces%", 2).await;
    barrier.rollback().await.unwrap();
    let left = left.await.unwrap();
    let right = right.await.unwrap();
    assert_eq!(left.0, StatusCode::OK);
    assert_eq!(right.0, StatusCode::OK);
    assert_eq!(
        left.1["deleted"].as_array().unwrap().len() + right.1["deleted"].as_array().unwrap().len(),
        1
    );
    assert!(
        f.list("one", "person-a").await["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let payload = support::save("post-purge", "one", support::context(json!(9), exact(4)), 1);
    assert!(
        !saved_id(
            &f.f.run(&f.client_actor, "observations.save", payload)
                .await
                .unwrap()
        )
        .is_nil()
    );
}

async fn wait_for_locks(f: &SubjectFixture, pattern: &str, count: i64) {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let waiting: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_stat_activity WHERE datname=current_database()
                AND wait_event_type='Lock' AND query LIKE $1",
            )
            .bind(pattern)
            .fetch_one(f.f.db.pool())
            .await
            .unwrap();
            if waiting >= count {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("both real competing requests reached the namespace mutex");
}
