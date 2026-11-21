//! Child of the identity HTTP fixture: actual session/authority HTTP and PostgreSQL.
use super::*;
use crate::{
    adapters::setup::{postgres::approved_destination, tests::DnsFixture},
    application::setup::*,
};
use serde_json::Value;

fn retry() -> Retry {
    Retry {
        key: Uuid::new_v4(),
        created_at: Utc::now(),
    }
}
async fn principal(f: &Fixture) -> TenantPrincipal {
    f.service
        .company(&f.token, f.upstream.company, Permission::ManageIntegrations)
        .await
        .unwrap()
}
async fn create_store(f: &Fixture, p: &TenantPrincipal, name: &str) -> StoreId {
    let r = f
        .setup
        .store
        .mutate(
            p,
            Mutation::CreateStore(StoreWrite {
                name: name.into(),
                timezone: "UTC".into(),
            }),
            retry(),
            &f.setup.dns,
        )
        .await
        .unwrap();
    serde_json::from_value(r.body["id"].clone()).unwrap()
}
async fn issue(f: &Fixture, p: &TenantPrincipal, store: StoreId) -> Value {
    f.setup
        .store
        .mutate(
            p,
            Mutation::CreateCredential(CredentialWrite {
                store_id: store,
                label: "Backend".into(),
                scopes: vec![KeyScope::ConversionsWrite, KeyScope::CheckoutsWrite],
                expires_at: None,
            }),
            retry(),
            &f.setup.dns,
        )
        .await
        .unwrap()
        .body
}
fn merchant_headers(secret: &str) -> axum::http::HeaderMap {
    let mut h = axum::http::HeaderMap::new();
    h.insert("authorization", format!("Bearer {secret}").parse().unwrap());
    h
}
async fn body(response: axum::response::Response) -> (StatusCode, Value) {
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 256 * 1024)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}
fn mutation_request(f: &Fixture, path: &str, payload: impl Into<Body>) -> Request<Body> {
    f.request(path)
        .method("POST")
        .header("x-company-id", f.upstream.company.0.to_string())
        .header("origin", "http://localhost:8091")
        .header("x-csrf-token", &f.csrf)
        .header("idempotency-key", Uuid::new_v4().to_string())
        .header("idempotency-created-at", Utc::now().to_rfc3339())
        .body(payload.into())
        .unwrap()
}

#[tokio::test]
async fn attribution_installation_http_roles_csrf_strict_json_and_limits() {
    let f = Fixture::new().await;
    let payload = r#"{"name":"Store <script>","timezone":"Europe/Berlin"}"#;
    let (status, value) = body(
        f.app
            .clone()
            .oneshot(mutation_request(&f, "/v1/stores", payload))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(value["name"], "Store <script>");
    let listing = f
        .request("/v1/stores")
        .header("x-company-id", f.upstream.company.0.to_string())
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        body(f.app.clone().oneshot(listing).await.unwrap()).await.1["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    for invalid in [
        r#"{"name":"a","name":"b","timezone":"UTC"}"#,
        r#"{"name":"a","timezone":"UTC","extra":true}"#,
        r#"{"name":"a","timezone":"not/a/zone"}"#,
    ] {
        assert_eq!(
            f.app
                .clone()
                .oneshot(mutation_request(&f, "/v1/stores", invalid))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        f.app
            .clone()
            .oneshot(mutation_request(&f, "/v1/stores", " ".repeat(16385)))
            .await
            .unwrap()
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let mut invalid = mutation_request(&f, "/v1/stores", payload);
    invalid
        .headers_mut()
        .insert("x-csrf-token", "wrong".parse().unwrap());
    assert_eq!(
        f.app.clone().oneshot(invalid).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    f.upstream.mode.store(3, Ordering::Release);
    assert_eq!(
        f.app
            .clone()
            .oneshot(mutation_request(&f, "/v1/stores", payload))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let listing = f
        .request("/v1/stores")
        .header("x-company-id", f.upstream.company.0.to_string())
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        f.app.clone().oneshot(listing).await.unwrap().status(),
        StatusCode::OK
    );
    f.close().await;
}

#[tokio::test]
async fn attribution_installation_concurrent_receipts_conflicts_and_pruned_horizon() {
    let f = Fixture::new().await;
    let p = principal(&f).await;
    let command = Mutation::CreateStore(StoreWrite {
        name: "First".into(),
        timezone: "UTC".into(),
    });
    let r = retry();
    let a = f.setup.store.mutate(
        &p,
        command.clone(),
        Retry {
            key: r.key,
            created_at: r.created_at,
        },
        &f.setup.dns,
    );
    let b = f.setup.store.mutate(
        &p,
        command.clone(),
        Retry {
            key: r.key,
            created_at: r.created_at,
        },
        &f.setup.dns,
    );
    let (a, b) = tokio::join!(a, b);
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_ne!(a.created, b.created);
    assert_eq!(a.body, b.body);
    for table in [
        "attribution_stores",
        "attribution_audit",
        "attribution_management_receipts",
    ] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&f._database.pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }
    let changed = Mutation::CreateStore(StoreWrite {
        name: "Other".into(),
        timezone: "UTC".into(),
    });
    assert!(matches!(
        f.setup
            .store
            .mutate(
                &p,
                changed,
                Retry {
                    key: r.key,
                    created_at: r.created_at
                },
                &f.setup.dns
            )
            .await,
        Err(Failure::Conflict)
    ));
    assert!(matches!(
        f.setup
            .store
            .mutate(
                &p,
                command.clone(),
                Retry {
                    key: r.key,
                    created_at: r.created_at + chrono::Duration::seconds(1)
                },
                &f.setup.dns
            )
            .await,
        Err(Failure::Conflict)
    ));
    sqlx::query("DELETE FROM attribution_management_receipts")
        .execute(&f._database.pool)
        .await
        .unwrap();
    assert!(matches!(
        f.setup
            .store
            .mutate(
                &p,
                command.clone(),
                Retry {
                    key: r.key,
                    created_at: Utc::now() - chrono::Duration::hours(25)
                },
                &f.setup.dns
            )
            .await,
        Err(Failure::Gone)
    ));
    assert!(matches!(
        f.setup
            .store
            .mutate(
                &p,
                command,
                Retry {
                    key: Uuid::new_v4(),
                    created_at: Utc::now() + chrono::Duration::seconds(31)
                },
                &f.setup.dns
            )
            .await,
        Err(Failure::Invalid)
    ));
    f.close().await;
}

#[tokio::test]
async fn attribution_installation_scope_keysets_and_foreign_constraints() {
    let f = Fixture::new().await;
    let p = principal(&f).await;
    let a = create_store(&f, &p, "A").await;
    let b = create_store(&f, &p, "B").await;
    let page = f
        .setup
        .store
        .list(
            &p,
            Resource::Stores,
            ListQuery {
                limit: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let cursor = page["next_cursor"].as_str().unwrap().to_owned();
    let second = f
        .setup
        .store
        .list(
            &p,
            Resource::Stores,
            ListQuery {
                limit: Some(1),
                cursor: Some(cursor.clone()),
                store_id: None,
            },
        )
        .await
        .unwrap();
    assert_ne!(page["items"][0]["id"], second["items"][0]["id"]);
    assert!(second["next_cursor"].is_null());
    assert!(matches!(
        f.setup
            .store
            .list(
                &p,
                Resource::Credentials,
                ListQuery {
                    cursor: Some(cursor.clone()),
                    ..Default::default()
                }
            )
            .await,
        Err(Failure::Invalid)
    ));
    assert!(matches!(
        f.setup
            .store
            .list(
                &p,
                Resource::Stores,
                ListQuery {
                    cursor: Some(cursor),
                    store_id: Some(a),
                    ..Default::default()
                }
            )
            .await,
        Err(Failure::Invalid)
    ));
    let domain = f
        .setup
        .store
        .mutate(
            &p,
            Mutation::CreateDomain(DomainWrite {
                store_id: a,
                host: "shop.merchant.com".into(),
            }),
            retry(),
            &f.setup.dns,
        )
        .await
        .unwrap()
        .body;
    let d: DomainId = serde_json::from_value(domain["domain"]["id"].clone()).unwrap();
    let invalid = f
        .setup
        .store
        .mutate(
            &p,
            Mutation::CreateInstallation(InstallationWrite {
                store_id: b,
                domain_id: d,
                name: "bad".into(),
            }),
            retry(),
            &f.setup.dns,
        )
        .await;
    assert!(matches!(invalid, Err(Failure::NotFound)));
    let constraint=sqlx::query("INSERT INTO attribution_installations(id,company_id,store_id,domain_id,name) VALUES($1,$2,$3,$4,'bypass')").bind(Uuid::new_v4()).bind(f.upstream.company.0).bind(b.0).bind(d.0).execute(&f._database.pool).await.unwrap_err();
    assert!(
        constraint
            .as_database_error()
            .unwrap()
            .is_foreign_key_violation()
    );
    assert!(matches!(
        f.setup
            .store
            .read(&p, Resource::Stores, Uuid::new_v4())
            .await,
        Err(Failure::NotFound)
    ));
    let foreign_company = Uuid::new_v4();
    let foreign_store = Uuid::new_v4();
    sqlx::query("INSERT INTO attribution_tenants(company_id,version,state,changed_at) VALUES($1,1,'active',clock_timestamp())").bind(foreign_company).execute(&f._database.pool).await.unwrap();
    sqlx::query(
        "INSERT INTO attribution_stores(id,company_id,name,timezone) VALUES($1,$2,'Foreign','UTC')",
    )
    .bind(foreign_store)
    .bind(foreign_company)
    .execute(&f._database.pool)
    .await
    .unwrap();
    assert!(matches!(
        f.setup
            .store
            .read(&p, Resource::Stores, foreign_store)
            .await,
        Err(Failure::NotFound)
    ));
    assert!(matches!(
        f.setup
            .store
            .mutate(
                &p,
                Mutation::CreateDomain(DomainWrite {
                    store_id: StoreId(foreign_store),
                    host: "foreign.merchant.com".into()
                }),
                retry(),
                &f.setup.dns
            )
            .await,
        Err(Failure::NotFound)
    ));
    f.close().await;
}

#[tokio::test]
async fn attribution_destination_verification_expiry_revoke_and_exact_scope() {
    let f = Fixture::new().await;
    let p = principal(&f).await;
    let store = create_store(&f, &p, "Shop").await;
    let dns = DnsFixture::new().await;
    let challenge = f
        .setup
        .store
        .mutate(
            &p,
            Mutation::CreateDomain(DomainWrite {
                store_id: store,
                host: "BÜCHER.de".into(),
            }),
            retry(),
            &dns.dns,
        )
        .await
        .unwrap()
        .body;
    let id: DomainId = serde_json::from_value(challenge["domain"]["id"].clone()).unwrap();
    dns.proof(
        "xn--bcher-kva.de",
        challenge["record_value"].as_str().unwrap(),
    );
    let r = retry();
    let original = r.created_at;
    let key = r.key;
    let verified = f
        .setup
        .store
        .mutate(&p, Mutation::VerifyDomain(id), r, &dns.dns)
        .await
        .unwrap()
        .body;
    assert_eq!(verified["status"], "verified");
    dns.mode.store(2, Ordering::Release);
    assert_eq!(
        f.setup
            .store
            .mutate(
                &p,
                Mutation::VerifyDomain(id),
                Retry {
                    key,
                    created_at: original
                },
                &dns.dns
            )
            .await
            .unwrap()
            .body,
        verified
    );
    let mut tx = f.setup.store.user_transaction(&p).await.unwrap();
    assert!(
        approved_destination(
            &mut tx,
            p.access.company_id,
            store,
            "https://BÜCHER.de/path"
        )
        .await
        .is_ok()
    );
    assert!(matches!(
        approved_destination(
            &mut tx,
            p.access.company_id,
            store,
            "https://xn--bcher-kva.de.attacker.com/"
        )
        .await,
        Err(Failure::NotFound)
    ));
    tx.rollback().await.unwrap();
    let install = f
        .setup
        .store
        .mutate(
            &p,
            Mutation::CreateInstallation(InstallationWrite {
                store_id: store,
                domain_id: id,
                name: "Web".into(),
            }),
            retry(),
            &dns.dns,
        )
        .await
        .unwrap()
        .body;
    let public: InstallationId = serde_json::from_value(install["id"].clone()).unwrap();
    let mut tx = f._database.pool.begin().await.unwrap();
    let public_config = f
        .setup
        .store
        .public_installation(&mut tx, public, p.access.company_id, &p.ticket)
        .await
        .unwrap();
    assert_eq!(
        public_config.allowed_observations,
        ["image_load", "landing_page_load", "visible_placement"]
    );
    tx.rollback().await.unwrap();
    sqlx::query("UPDATE attribution_domains SET verified_until=clock_timestamp()-interval '1 second' WHERE id=$1").bind(id.0).execute(&f._database.pool).await.unwrap();
    let mut tx = f._database.pool.begin().await.unwrap();
    assert!(matches!(
        f.setup
            .store
            .public_installation(&mut tx, public, p.access.company_id, &p.ticket)
            .await,
        Err(Failure::Gone)
    ));
    tx.rollback().await.unwrap();
    dns.mode.store(0, Ordering::Release);
    f.setup
        .store
        .mutate(&p, Mutation::VerifyDomain(id), retry(), &dns.dns)
        .await
        .unwrap();
    f.setup
        .store
        .mutate(&p, Mutation::RevokeDomain(id), retry(), &dns.dns)
        .await
        .unwrap();
    assert!(matches!(
        f.setup
            .store
            .mutate(&p, Mutation::VerifyDomain(id), retry(), &dns.dns)
            .await,
        Err(Failure::Gone)
    ));
    let replacement = f
        .setup
        .store
        .mutate(
            &p,
            Mutation::CreateDomain(DomainWrite {
                store_id: store,
                host: "bücher.de".into(),
            }),
            retry(),
            &dns.dns,
        )
        .await
        .unwrap()
        .body;
    assert_ne!(replacement["record_value"], challenge["record_value"]);
    f.close().await;
}

#[tokio::test]
async fn attribution_credentials_once_replay_rotation_revocation_and_hash_only() {
    let f = Fixture::new().await;
    let p = principal(&f).await;
    let store = create_store(&f, &p, "Shop").await;
    let command = Mutation::CreateCredential(CredentialWrite {
        store_id: store,
        label: "Server".into(),
        scopes: vec![KeyScope::ConversionsWrite],
        expires_at: None,
    });
    let r = retry();
    let (first, replay) = tokio::join!(
        f.setup.store.mutate(
            &p,
            command.clone(),
            Retry {
                key: r.key,
                created_at: r.created_at
            },
            &f.setup.dns
        ),
        f.setup.store.mutate(
            &p,
            command,
            Retry {
                key: r.key,
                created_at: r.created_at
            },
            &f.setup.dns
        )
    );
    let first = first.unwrap();
    let replay = replay.unwrap();
    let (first, replay) = if first.created {
        (first, replay)
    } else {
        (replay, first)
    };
    let secret = first.body["secret"].as_str().unwrap();
    assert_eq!(secret.len(), 46);
    assert!(secret.starts_with("ak_"));
    assert!(replay.body.get("secret").is_none());
    assert_eq!(replay.body["secret_available"], false);
    let credential: CredentialId =
        serde_json::from_value(first.body["credential"]["id"].clone()).unwrap();
    for table in [
        "attribution_credentials",
        "attribution_management_receipts",
        "attribution_audit",
    ] {
        let rows: Vec<Value> = sqlx::query_scalar(&format!("SELECT to_jsonb(r) FROM {table} r"))
            .fetch_all(&f._database.pool)
            .await
            .unwrap();
        assert!(!serde_json::to_string(&rows).unwrap().contains(secret));
    }
    let stored: Vec<u8> =
        sqlx::query_scalar("SELECT verifier FROM attribution_credentials WHERE id=$1")
            .bind(credential.0)
            .fetch_one(&f._database.pool)
            .await
            .unwrap();
    assert_eq!(stored, crypto::verifier(secret));
    let headers = merchant_headers(secret);
    let merchant = f
        .setup
        .store
        .merchant(
            &f.service,
            &f.setup.admission,
            &headers,
            KeyScope::ConversionsWrite,
        )
        .await
        .unwrap();
    let tx = f.setup.store.begin_merchant(&merchant).await.unwrap();
    tx.commit().await.unwrap();
    assert!(
        f.setup
            .store
            .read(&p, Resource::Credentials, credential.0)
            .await
            .unwrap()["last_used_at"]
            .is_string()
    );
    let r = retry();
    let rotated = f
        .setup
        .store
        .mutate(
            &p,
            Mutation::RotateCredential(
                credential,
                RotateCredential {
                    overlap_seconds: 60,
                },
            ),
            Retry {
                key: r.key,
                created_at: r.created_at,
            },
            &f.setup.dns,
        )
        .await
        .unwrap()
        .body;
    let old = f
        .setup
        .store
        .read(&p, Resource::Credentials, credential.0)
        .await
        .unwrap();
    let new_id: CredentialId = serde_json::from_value(rotated["credential"]["id"].clone()).unwrap();
    f.setup
        .store
        .mutate(
            &p,
            Mutation::RevokeCredential(new_id),
            retry(),
            &f.setup.dns,
        )
        .await
        .unwrap();
    let replay = f
        .setup
        .store
        .mutate(
            &p,
            Mutation::RotateCredential(
                credential,
                RotateCredential {
                    overlap_seconds: 60,
                },
            ),
            r,
            &f.setup.dns,
        )
        .await
        .unwrap()
        .body;
    assert_eq!(replay["credential"]["id"], rotated["credential"]["id"]);
    assert!(replay.get("secret").is_none());
    assert_eq!(
        f.setup
            .store
            .read(&p, Resource::Credentials, credential.0)
            .await
            .unwrap()["expires_at"],
        old["expires_at"]
    );
    f.setup
        .store
        .merchant(
            &f.service,
            &f.setup.admission,
            &headers,
            KeyScope::ConversionsWrite,
        )
        .await
        .unwrap();
    f.setup
        .store
        .mutate(
            &p,
            Mutation::RevokeCredential(credential),
            retry(),
            &f.setup.dns,
        )
        .await
        .unwrap();
    assert!(matches!(
        f.setup.store.begin_merchant(&merchant).await,
        Err(Failure::Unauthorized)
    ));
    f.close().await;
}

#[tokio::test]
async fn attribution_credentials_auth_classes_scope_expiry_store_and_company() {
    let f = Fixture::new().await;
    let p = principal(&f).await;
    let store = create_store(&f, &p, "Shop").await;
    let issued = issue(&f, &p, store).await;
    let secret = issued["secret"].as_str().unwrap();
    for invalid in [
        crypto::random(),
        Uuid::new_v4().to_string(),
        format!("ak_{}", crypto::random()),
    ] {
        assert!(matches!(
            f.setup
                .store
                .merchant(
                    &f.service,
                    &f.setup.admission,
                    &merchant_headers(&invalid),
                    KeyScope::ConversionsWrite
                )
                .await,
            Err(Failure::Unauthorized)
        ));
    }
    let mut h = merchant_headers(secret);
    h.insert("cookie", "__Host-attribution=invalid".parse().unwrap());
    assert!(matches!(
        f.setup
            .store
            .merchant(
                &f.service,
                &f.setup.admission,
                &h,
                KeyScope::ConversionsWrite
            )
            .await,
        Err(Failure::Unauthorized)
    ));
    let h = merchant_headers(secret);
    assert!(matches!(
        f.setup
            .store
            .merchant(&f.service, &f.setup.admission, &h, KeyScope::AnalyticsRead)
            .await,
        Err(Failure::Forbidden)
    ));
    let mut foreign = h.clone();
    foreign.insert("x-company-id", Uuid::new_v4().to_string().parse().unwrap());
    assert!(matches!(
        f.setup
            .store
            .merchant(
                &f.service,
                &f.setup.admission,
                &foreign,
                KeyScope::ConversionsWrite
            )
            .await,
        Err(Failure::NotFound)
    ));
    let merchant = f
        .setup
        .store
        .merchant(
            &f.service,
            &f.setup.admission,
            &h,
            KeyScope::ConversionsWrite,
        )
        .await
        .unwrap();
    assert!(matches!(
        merchant.store(StoreId(Uuid::new_v4())),
        Err(Failure::NotFound)
    ));
    f.setup
        .store
        .mutate(
            &p,
            Mutation::UpdateStore(
                store,
                StoreUpdate {
                    name: None,
                    timezone: None,
                    enabled: Some(false),
                },
            ),
            retry(),
            &f.setup.dns,
        )
        .await
        .unwrap();
    assert!(matches!(
        f.setup.store.begin_merchant(&merchant).await,
        Err(Failure::Forbidden)
    ));
    f.setup
        .store
        .mutate(
            &p,
            Mutation::UpdateStore(
                store,
                StoreUpdate {
                    name: None,
                    timezone: None,
                    enabled: Some(true),
                },
            ),
            retry(),
            &f.setup.dns,
        )
        .await
        .unwrap();
    sqlx::query(
        "UPDATE attribution_credentials SET expires_at=clock_timestamp()-interval '1 second'",
    )
    .execute(&f._database.pool)
    .await
    .unwrap();
    assert!(matches!(
        f.setup
            .store
            .merchant(
                &f.service,
                &f.setup.admission,
                &h,
                KeyScope::ConversionsWrite
            )
            .await,
        Err(Failure::Unauthorized)
    ));
    f.close().await;
}

#[tokio::test]
async fn attribution_credentials_competing_revoke_and_use_serializes_both_orders() {
    let f = Fixture::new().await;
    let p = principal(&f).await;
    let store = create_store(&f, &p, "Shop").await;
    let issued = issue(&f, &p, store).await;
    let credential: CredentialId =
        serde_json::from_value(issued["credential"]["id"].clone()).unwrap();
    let h = merchant_headers(issued["secret"].as_str().unwrap());
    let merchant = f
        .setup
        .store
        .merchant(
            &f.service,
            &f.setup.admission,
            &h,
            KeyScope::ConversionsWrite,
        )
        .await
        .unwrap();
    {
        let tx = f.setup.store.begin_merchant(&merchant).await.unwrap();
        let revoke = f.setup.store.mutate(
            &p,
            Mutation::RevokeCredential(credential),
            retry(),
            &f.setup.dns,
        );
        tokio::pin!(revoke);
        assert!(
            tokio::time::timeout(Duration::from_millis(80), &mut revoke)
                .await
                .is_err()
        );
        tx.commit().await.unwrap();
        revoke.await.unwrap();
    }
    assert!(matches!(
        f.setup.store.begin_merchant(&merchant).await,
        Err(Failure::Unauthorized)
    ));
    let issued = issue(&f, &p, store).await;
    let credential: CredentialId =
        serde_json::from_value(issued["credential"]["id"].clone()).unwrap();
    let h = merchant_headers(issued["secret"].as_str().unwrap());
    let merchant = f
        .setup
        .store
        .merchant(
            &f.service,
            &f.setup.admission,
            &h,
            KeyScope::ConversionsWrite,
        )
        .await
        .unwrap();
    {
        let mut revoke_tx = f.setup.store.user_transaction(&p).await.unwrap();
        sqlx::query("UPDATE attribution_credentials SET revoked_at=clock_timestamp() WHERE id=$1")
            .bind(credential.0)
            .execute(&mut *revoke_tx)
            .await
            .unwrap();
        let use_key = f.setup.store.begin_merchant(&merchant);
        tokio::pin!(use_key);
        assert!(
            tokio::time::timeout(Duration::from_millis(80), &mut use_key)
                .await
                .is_err()
        );
        revoke_tx.commit().await.unwrap();
        assert!(matches!(use_key.await, Err(Failure::Unauthorized)));
    }
    f.close().await;
}

#[tokio::test]
async fn attribution_credentials_logout_and_authority_recheck_before_replay() {
    let f = Fixture::new().await;
    let p = principal(&f).await;
    let request = mutation_request(&f, "/v1/stores", r#"{"name":"Shop","timezone":"UTC"}"#);
    let headers = request.headers().clone();
    let response = f.app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    f.upstream.mode.store(3, Ordering::Release);
    let mut request = Request::builder()
        .method("POST")
        .uri("/v1/stores")
        .body(Body::from(r#"{"name":"Shop","timezone":"UTC"}"#))
        .unwrap();
    *request.headers_mut() = headers;
    assert_eq!(
        f.app.clone().oneshot(request).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    f.store
        .revoke_session(p.principal.session.id)
        .await
        .unwrap();
    assert!(matches!(
        f.setup
            .store
            .mutate(
                &p,
                Mutation::CreateStore(StoreWrite {
                    name: "Rejected".into(),
                    timezone: "UTC".into()
                }),
                retry(),
                &f.setup.dns
            )
            .await,
        Err(Failure::Unauthorized)
    ));
    f.close().await;
}

#[tokio::test]
async fn attribution_destination_competing_verification_and_revocation() {
    let f = Fixture::new().await;
    let p = principal(&f).await;
    let store = create_store(&f, &p, "Shop").await;
    let dns = DnsFixture::new().await;
    let challenge = f
        .setup
        .store
        .mutate(
            &p,
            Mutation::CreateDomain(DomainWrite {
                store_id: store,
                host: "shop.merchant.com".into(),
            }),
            retry(),
            &dns.dns,
        )
        .await
        .unwrap()
        .body;
    let id: DomainId = serde_json::from_value(challenge["domain"]["id"].clone()).unwrap();
    dns.proof(
        "shop.merchant.com",
        challenge["record_value"].as_str().unwrap(),
    );
    dns.mode.store(4, Ordering::Release);
    {
        let verify = f
            .setup
            .store
            .mutate(&p, Mutation::VerifyDomain(id), retry(), &dns.dns);
        tokio::pin!(verify);
        tokio::select! { result=&mut verify=>panic!("verification should wait: {}",result.is_ok()), _=dns.seen.notified()=>{} }
        f.setup
            .store
            .mutate(&p, Mutation::RevokeDomain(id), retry(), &dns.dns)
            .await
            .unwrap();
        dns.mode.store(0, Ordering::Release);
        dns.release.notify_one();
        assert!(matches!(verify.await, Err(Failure::Gone)));
    }
    assert_eq!(
        f.setup
            .store
            .list(&p, Resource::Domains, ListQuery::default())
            .await
            .unwrap()["items"][0]["status"],
        "revoked"
    );
    f.close().await;
}

#[tokio::test]
async fn attribution_credentials_header_auth_precedes_body_and_freshness_fails_closed() {
    let f = Fixture::new().await;
    let p = principal(&f).await;
    let store = create_store(&f, &p, "Shop").await;
    let issued = issue(&f, &p, store).await;
    let secret = issued["secret"].as_str().unwrap();
    let setup = f.setup.clone();
    let app=Router::new().route("/fixture/ingest",post(move |request:axum::extract::Request| {
        let setup=setup.clone();async move {
            let principal=setup.store.merchant(&setup.identity.service,&setup.admission,request.headers(),KeyScope::ConversionsWrite).await?;
            let bytes=axum::body::to_bytes(request.into_body(),256*1024).await.map_err(|_|Failure::TooLarge)?;
            let input:StoreWrite=serde_json::from_slice(&bytes).map_err(|_|Failure::Invalid)?;
            let mut tx=setup.store.begin_merchant(&principal).await?;
            // A fixture side effect in the real transaction proves failure rolls
            // back last-use and cannot acknowledge an unauthorized accepted write.
            sqlx::query("INSERT INTO attribution_audit(id,company_id,actor_kind,actor_id,action,resource_id) VALUES($1,$2,'merchant',$3,'fixture.ingest',$4)").bind(Uuid::new_v4()).bind(principal.company.0).bind(principal.credential.0).bind(principal.store.0).execute(&mut *tx).await.map_err(|_|Failure::Unavailable)?;
            assert_eq!(input.name,"fixture");tx.commit().await.map_err(|_|Failure::Unavailable)?;
            Ok::<_,Failure>(StatusCode::NO_CONTENT)
        }
    }));
    for key in [None, Some("public-installation"), Some("ak_invalid")] {
        for input in ["invalid".into(), " ".repeat(256 * 1024 + 1)] {
            let mut builder = Request::builder().uri("/fixture/ingest").method("POST");
            if let Some(key) = key {
                builder = builder.header("authorization", format!("Bearer {key}"));
            }
            assert_eq!(
                app.clone()
                    .oneshot(builder.body(Body::from(input)).unwrap())
                    .await
                    .unwrap()
                    .status(),
                StatusCode::UNAUTHORIZED
            );
        }
    }
    let make = |body: &str| {
        Request::builder()
            .uri("/fixture/ingest")
            .method("POST")
            .header("authorization", format!("Bearer {secret}"))
            .body(Body::from(body.to_owned()))
            .unwrap()
    };
    assert_eq!(
        app.clone().oneshot(make("invalid")).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        app.clone()
            .oneshot(make(&" ".repeat(256 * 1024 + 1)))
            .await
            .unwrap()
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert!(
        f.setup
            .store
            .list(&p, Resource::Credentials, ListQuery::default())
            .await
            .unwrap()["items"][0]["last_used_at"]
            .is_null()
    );
    let calls = f.upstream.calls.load(Ordering::Acquire);
    assert_eq!(
        app.clone()
            .oneshot(make(r#"{"name":"fixture","timezone":"UTC"}"#))
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        f.upstream.calls.load(Ordering::Acquire),
        calls,
        "fresh merchant requests must not call authority per event"
    );
    f.service.freshness.invalidate(f.upstream.company);
    f.upstream.mode.store(2, Ordering::Release);
    assert_eq!(
        app.clone().oneshot(make("invalid")).await.unwrap().status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    f.upstream.mode.store(0, Ordering::Release);
    f.store
        .observe(
            &CompanyStatus {
                company_id: f.upstream.company,
                version: 2,
                state: LifecycleState::Suspended,
                changed_at: Utc::now(),
            },
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        app.clone().oneshot(make("invalid")).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    f.close().await;
}

#[tokio::test]
#[ignore = "Requires pinned Playwright Chromium; CI and the final gate run explicitly"]
async fn attribution_setup_browser_real_forms_and_postgres() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let mut f = Fixture::with_origin(&origin).await;
    let dns = DnsFixture::new().await;
    f.setup.dns = dns.dns.clone();
    let records = dns.records.clone();
    let pool = f._database.pool.clone();
    let role = f.upstream.mode.clone();
    let app = router(
        HttpState::new(
            f._database.pool.clone(),
            Arc::new(std::sync::atomic::AtomicBool::new(true)),
            false,
        )
        .with_identity(f.setup.identity.clone())
        .with_setup(f.setup.clone())
        .with_commerce(Fixture::commerce_http(&f.setup)),
    )
    .route(
        "/fixture/publish-dns",
        post(move || {
            let records = records.clone();
            let pool = pool.clone();
            async move {
                let rows: Vec<(String, String)> =
                    sqlx::query_as("SELECT host,challenge FROM attribution_domains")
                        .fetch_all(&pool)
                        .await
                        .unwrap();
                for (host, challenge) in rows {
                    records
                        .write()
                        .unwrap()
                        .insert(format!("_attribution.{host}."), challenge);
                }
                StatusCode::NO_CONTENT
            }
        }),
    )
    .route(
        "/fixture/member",
        post(move || {
            let role = role.clone();
            async move {
                role.store(3, Ordering::Release);
                StatusCode::NO_CONTENT
            }
        }),
    );
    let shutdown = CancellationToken::new();
    let stop = shutdown.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(stop.cancelled_owned())
            .await
    });
    let output = tokio::time::timeout(
        Duration::from_secs(90),
        tokio::process::Command::new("node")
            .arg("browser/setup.mjs")
            .env("ATTRIBUTION_BROWSER_ORIGIN", origin)
            .env("ATTRIBUTION_BROWSER_SESSION", &f.token.0)
            .env(
                "ATTRIBUTION_BROWSER_COMPANY",
                f.upstream.company.0.to_string(),
            )
            .kill_on_drop(true)
            .output(),
    )
    .await;
    shutdown.cancel();
    server.await.unwrap().unwrap();
    let clicked: (i64, Option<String>) = sqlx::query_as(
        "SELECT count(*),min(dimensions->>'advertised_product_id') FROM attribution_clicks",
    )
    .fetch_one(&f._database.pool)
    .await
    .unwrap();
    f.close().await;
    let output = output
        .expect("browser fixture deadline")
        .expect("node must be installed");
    println!("{}", String::from_utf8_lossy(&output.stdout));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        clicked,
        (1, Some("P2".into())),
        "browser navigation must commit its click with edited product snapshot"
    );
}

#[tokio::test]
async fn attribution_credentials_expiry_overlap_and_expired_metadata_replay() {
    let f = Fixture::new().await;
    let p = principal(&f).await;
    let store = create_store(&f, &p, "Shop").await;
    let r = retry();
    let expiry = Utc::now() + chrono::Duration::minutes(5);
    let mutation = Mutation::CreateCredential(CredentialWrite {
        store_id: store,
        label: "Expiring".into(),
        scopes: vec![KeyScope::ConversionsWrite],
        expires_at: Some(expiry),
    });
    let issued = f
        .setup
        .store
        .mutate(
            &p,
            mutation.clone(),
            Retry {
                key: r.key,
                created_at: r.created_at,
            },
            &f.setup.dns,
        )
        .await
        .unwrap()
        .body;
    let old: Credential = serde_json::from_value(issued["credential"].clone()).unwrap();
    let rotated = f
        .setup
        .store
        .mutate(
            &p,
            Mutation::RotateCredential(
                old.id,
                RotateCredential {
                    overlap_seconds: 86400,
                },
            ),
            retry(),
            &f.setup.dns,
        )
        .await
        .unwrap()
        .body;
    let new: Credential = serde_json::from_value(rotated["credential"].clone()).unwrap();
    let old_after: Credential = serde_json::from_value(
        f.setup
            .store
            .read(&p, Resource::Credentials, old.id.0)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(old.expires_at, old_after.expires_at);
    assert_eq!(old.expires_at, new.expires_at);
    assert!(matches!(
        f.setup
            .store
            .mutate(
                &p,
                Mutation::RotateCredential(old.id, RotateCredential { overlap_seconds: 1 }),
                retry(),
                &f.setup.dns
            )
            .await,
        Err(Failure::Conflict)
    ));
    f.setup
        .store
        .mutate(
            &p,
            Mutation::RotateCredential(new.id, RotateCredential { overlap_seconds: 0 }),
            retry(),
            &f.setup.dns,
        )
        .await
        .unwrap();
    assert!(matches!(
        f.setup
            .store
            .merchant(
                &f.service,
                &f.setup.admission,
                &merchant_headers(rotated["secret"].as_str().unwrap()),
                KeyScope::ConversionsWrite
            )
            .await,
        Err(Failure::Unauthorized)
    ));
    sqlx::query("UPDATE attribution_credentials SET expires_at=clock_timestamp()-interval '1 second' WHERE id=$1").bind(old.id.0).execute(&f._database.pool).await.unwrap();
    assert_eq!(
        f.setup
            .store
            .mutate(&p, mutation, r, &f.setup.dns)
            .await
            .unwrap()
            .body["secret_available"],
        false
    );
    f.close().await;
}

#[tokio::test]
async fn attribution_installation_runtime_shapes_and_rejected_nullable_payloads() {
    let f = Fixture::new().await;
    let p = principal(&f).await;
    let store = create_store(&f, &p, "Shop").await;
    let _: Store = serde_json::from_value(
        f.setup
            .store
            .read(&p, Resource::Stores, store.0)
            .await
            .unwrap(),
    )
    .unwrap();
    let page: Page<Store> = serde_json::from_value(
        f.setup
            .store
            .list(&p, Resource::Stores, ListQuery::default())
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(page.items.len(), 1);
    let domain = f
        .setup
        .store
        .mutate(
            &p,
            Mutation::CreateDomain(DomainWrite {
                store_id: store,
                host: "shop.merchant.com".into(),
            }),
            retry(),
            &f.setup.dns,
        )
        .await
        .unwrap()
        .body;
    let parsed: Domain = serde_json::from_value(domain["domain"].clone()).unwrap();
    assert_eq!(parsed.status, "pending");
    assert!(domain["record_name"].as_str().unwrap().len() <= 253);
    assert_eq!(domain["record_value"].as_str().unwrap().len(), 43);
    assert!(serde_json::from_str::<StoreUpdate>(r#"{"enabled":null}"#).is_err());
    let issued = issue(&f, &p, store).await;
    let _: Credential = serde_json::from_value(issued["credential"].clone()).unwrap();
    for payload in [
        format!(
            r#"{{"store_id":"{}","label":"Missing expiry","scopes":["conversions:write"]}}"#,
            store.0
        ),
        format!(
            r#"{{"store_id":"{}","label":"Duplicate scope","scopes":["conversions:write","conversions:write"],"expires_at":null}}"#,
            store.0
        ),
    ] {
        assert_eq!(
            f.app
                .clone()
                .oneshot(mutation_request(&f, "/v1/credentials", payload))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    f.close().await;
}

async fn blocked_by(admin: &mut sqlx::PgConnection, holder: i32) -> i32 {
    tokio::time::timeout(Duration::from_secs(1),async {
        loop {
            if let Some(pid)=sqlx::query_scalar::<_,i32>("SELECT pid FROM pg_stat_activity WHERE datname=current_database() AND $1=ANY(pg_blocking_pids(pid)) LIMIT 1").bind(holder).fetch_optional(&mut *admin).await.unwrap(){return pid;}
            tokio::task::yield_now().await;
        }
    }).await.expect("production contender must actually wait on the held lock")
}

#[tokio::test]
async fn attribution_credentials_competing_store_disable_and_use_recheck_both_orders() {
    let f = Fixture::new().await;
    let p = principal(&f).await;
    let store = create_store(&f, &p, "Shop").await;
    let issued = issue(&f, &p, store).await;
    let credential: CredentialId =
        serde_json::from_value(issued["credential"]["id"].clone()).unwrap();
    let merchant = f
        .setup
        .store
        .merchant(
            &f.service,
            &f.setup.admission,
            &merchant_headers(issued["secret"].as_str().unwrap()),
            KeyScope::ConversionsWrite,
        )
        .await
        .unwrap();
    let disable = || {
        Mutation::UpdateStore(
            store,
            StoreUpdate {
                name: None,
                timezone: None,
                enabled: Some(false),
            },
        )
    };
    let mut admin = f._database.admin().await;
    {
        // Use wins: the production disable must wait until the accepted use's
        // last-use mutation commits, then blocks every later use.
        let mut accepted = f.setup.store.begin_merchant(&merchant).await.unwrap();
        let holder: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *accepted)
            .await
            .unwrap();
        let disabling = f.setup.store.mutate(&p, disable(), retry(), &f.setup.dns);
        tokio::pin!(disabling);
        tokio::select! {result=&mut disabling=>panic!("disable completed before use released: {}",result.is_ok()), _=blocked_by(&mut admin,holder)=>{}}
        accepted.commit().await.unwrap();
        disabling.await.unwrap();
        assert!(matches!(
            f.setup.store.begin_merchant(&merchant).await,
            Err(Failure::Forbidden)
        ));
    }
    f.setup
        .store
        .mutate(
            &p,
            Mutation::UpdateStore(
                store,
                StoreUpdate {
                    name: None,
                    timezone: None,
                    enabled: Some(true),
                },
            ),
            retry(),
            &f.setup.dns,
        )
        .await
        .unwrap();
    let previous = f
        .setup
        .store
        .read(&p, Resource::Credentials, credential.0)
        .await
        .unwrap()["last_used_at"]
        .clone();
    assert!(previous.is_string());
    {
        // Disable wins: pause its production user transaction on the session row
        // after it owns the tenant lock, then observe the merchant waiting behind it.
        let mut session_guard = f._database.pool.begin().await.unwrap();
        sqlx::query("SELECT id FROM attribution_sessions WHERE id=$1 FOR UPDATE")
            .bind(p.principal.session.id.0)
            .fetch_one(&mut *session_guard)
            .await
            .unwrap();
        let holder: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *session_guard)
            .await
            .unwrap();
        let disabling = f.setup.store.mutate(&p, disable(), retry(), &f.setup.dns);
        tokio::pin!(disabling);
        let disable_pid = tokio::select! {result=&mut disabling=>panic!("disable completed before session released: {}",result.is_ok()),pid=blocked_by(&mut admin,holder)=>pid};
        let using = f.setup.store.begin_merchant(&merchant);
        tokio::pin!(using);
        tokio::select! {result=&mut using=>panic!("use completed before disabling transaction: {}",result.is_ok()),_=blocked_by(&mut admin,disable_pid)=>{}}
        session_guard.commit().await.unwrap();
        let (disabled, used) = tokio::join!(disabling, using);
        disabled.unwrap();
        assert!(matches!(used, Err(Failure::Forbidden)));
    }
    assert_eq!(
        f.setup
            .store
            .read(&p, Resource::Stores, store.0)
            .await
            .unwrap()["enabled"],
        false
    );
    assert_eq!(
        f.setup
            .store
            .read(&p, Resource::Credentials, credential.0)
            .await
            .unwrap()["last_used_at"],
        previous,
        "denied waiter cannot commit last use"
    );
    drop(admin);
    f.close().await;
}

#[tokio::test]
async fn attribution_credentials_replay_after_original_payload_expiry() {
    let f = Fixture::new().await;
    let p = principal(&f).await;
    let store = create_store(&f, &p, "Shop").await;
    let expiry: chrono::DateTime<Utc> =
        sqlx::query_scalar("SELECT clock_timestamp()+interval '2 seconds'")
            .fetch_one(&f._database.pool)
            .await
            .unwrap();
    let mutation = Mutation::CreateCredential(CredentialWrite {
        store_id: store,
        label: "Short-lived".into(),
        scopes: vec![KeyScope::ConversionsWrite],
        expires_at: Some(expiry),
    });
    let original = retry();
    let issued = f
        .setup
        .store
        .mutate(
            &p,
            mutation.clone(),
            Retry {
                key: original.key,
                created_at: original.created_at,
            },
            &f.setup.dns,
        )
        .await
        .unwrap();
    assert!(issued.created);
    assert!(issued.body["secret"].is_string());
    let id = issued.body["credential"]["id"].clone();
    // Advance neither payload nor row: cross the original submitted deadline in
    // real database time, so a misplaced pre-receipt expiry check would fail.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let expired: bool = sqlx::query_scalar("SELECT clock_timestamp()>=$1")
                .bind(expiry)
                .fetch_one(&f._database.pool)
                .await
                .unwrap();
            if expired {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("database clock must cross original expiry within the fixture bound");
    let replay = f
        .setup
        .store
        .mutate(&p, mutation.clone(), original, &f.setup.dns)
        .await
        .unwrap();
    assert!(!replay.created);
    assert_eq!(replay.body["secret_available"], false);
    assert!(replay.body.get("secret").is_none());
    assert_eq!(replay.body["credential"]["id"], id);
    assert!(matches!(
        f.setup
            .store
            .mutate(&p, mutation, retry(), &f.setup.dns)
            .await,
        Err(Failure::Invalid)
    ));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM attribution_credentials")
        .fetch_one(&f._database.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    let receipts:i64=sqlx::query_scalar("SELECT count(*) FROM attribution_management_receipts WHERE operation='POST /v1/credentials'").fetch_one(&f._database.pool).await.unwrap();
    assert_eq!(receipts, 1);
    f.close().await;
}
