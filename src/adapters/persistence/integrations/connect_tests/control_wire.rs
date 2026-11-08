use super::*;
use crate::application::integrations::wire::{self, Command, Reply};

#[tokio::test]
async fn integration_service_contract_control_cannot_decode_owner_deadline_or_cancellation_into_authority()
 {
    let f = Box::pin(Fixture::new()).await;
    for fault in ["owner", "deadline", "expired", "cancelled", "version"] {
        let deadline =
            Utc::now() + chrono::Duration::seconds(if fault == "expired" { -1 } else { 20 });
        let request = wire::Request {
            version: wire::PROTOCOL_VERSION,
            identity: DispatchId::new(Uuid::new_v4()),
            owner: f.owner.owner().clone(),
            deadline,
            cancellation: CancellationId::parse(Uuid::new_v4().to_string()).unwrap(),
            command: Command::ListAccounts {
                page: wire::Page {
                    after: None,
                    size: 10,
                },
            },
        };
        let mut value = serde_json::to_value(&request).unwrap();
        match fault {
            "owner" => value["owner"]["subject"] = json!(Uuid::new_v4().to_string()),
            "deadline" => value["deadline"] = json!(deadline + chrono::Duration::seconds(1)),
            "version" => value["version"] = json!(wire::PROTOCOL_VERSION + 1),
            _ => {}
        }
        let cancel = CancellationToken::new();
        if fault == "cancelled" {
            cancel.cancel();
        }
        let local = wire::control_transport::LocalContext::new(
            &request,
            &f.owner,
            ConnectWork {
                deadline,
                cancellation: cancel,
            },
            None,
        )
        .unwrap();
        let altered: wire::Request = serde_json::from_value(value).unwrap();
        assert!(
            wire::control_transport::exchange(
                f.gateway().as_ref(),
                local,
                &serde_json::to_vec(&altered).unwrap(),
            )
            .await
            .is_err(),
            "{fault}"
        );
    }
    assert_eq!(f.script.calls.load(Ordering::SeqCst), 0);
}

async fn command(f: &Fixture, owner: &VerifiedOwner, command: Command) -> Reply {
    let deadline = Utc::now() + chrono::Duration::seconds(20);
    let request = wire::Request {
        version: wire::PROTOCOL_VERSION,
        identity: DispatchId::new(Uuid::new_v4()),
        owner: owner.owner().clone(),
        deadline,
        cancellation: CancellationId::parse(Uuid::new_v4().to_string()).unwrap(),
        command,
    };
    let local = wire::control_transport::LocalContext::new(
        &request,
        owner,
        ConnectWork {
            deadline,
            cancellation: CancellationToken::new(),
        },
        None,
    )
    .unwrap();
    let bytes =
        wire::control_transport::exchange(f.gateway().as_ref(), local, &request.encode().unwrap())
            .await
            .unwrap();
    for secret in [
        "wire-static-secret",
        "wire-static-replacement",
        "refresh-sentinel",
        "access-sentinel",
        "client-secret-sentinel",
        "enc:v2",
    ] {
        assert!(!String::from_utf8_lossy(&bytes).contains(secret));
    }
    wire::Response::decode(&bytes, request.identity)
        .unwrap()
        .reply
}

#[tokio::test]
async fn integration_service_contract_control_binds_each_identity_and_original_local_context() {
    let f = Box::pin(Fixture::new()).await;
    for connect in [false, true] {
        for fault in ["identity", "cancellation", "crossed", "command"] {
            assert_substitution_denied(&f, connect, fault).await;
        }
    }
    let Reply::Accounts { page } = command(
        &f,
        &f.owner,
        Command::ListAccounts {
            page: wire::Page {
                after: None,
                size: 10,
            },
        },
    )
    .await
    else {
        panic!("account metadata reply")
    };
    assert!(page.accounts.is_empty(), "no management work entered");
    assert_eq!(f.script.calls.load(Ordering::SeqCst), 0);
    assert!(matches!(
        command(
            &f,
            &f.owner,
            Command::CreateStatic {
                input: static_input("wire-static-secret"),
            }
        )
        .await,
        Reply::Account { .. }
    ));
}
async fn assert_substitution_denied(f: &Fixture, connect: bool, fault: &str) {
    let deadline = Utc::now() + chrono::Duration::seconds(20);
    let command = if connect {
        let cmd = super::command(None);
        Command::BeginConnect {
            app: cmd.app,
            config: cmd.config,
            capabilities: cmd.requested_capabilities,
            return_target: cmd.return_target,
            reconnect: None,
        }
    } else {
        Command::CreateStatic {
            input: static_input("wire-static-secret"),
        }
    };
    let mut request = wire::Request {
        version: wire::PROTOCOL_VERSION,
        identity: DispatchId::new(Uuid::new_v4()),
        owner: f.owner.owner().clone(),
        deadline,
        cancellation: CancellationId::parse(Uuid::new_v4().to_string()).unwrap(),
        command,
    };
    let local = wire::control_transport::LocalContext::new(
        &request,
        &f.owner,
        ConnectWork {
            deadline,
            cancellation: CancellationToken::new(),
        },
        None,
    )
    .unwrap();
    let original = request.encode().unwrap();
    if matches!(fault, "identity" | "crossed") {
        request.identity = DispatchId::new(Uuid::new_v4());
    }
    if matches!(fault, "cancellation" | "crossed") {
        request.cancellation = CancellationId::parse(Uuid::new_v4().to_string()).unwrap();
    }
    if fault == "command" {
        request.command = Command::ConnectableApps {};
    }
    if fault == "crossed" {
        let other = wire::control_transport::LocalContext::new(
            &request,
            &f.owner,
            ConnectWork {
                deadline,
                cancellation: CancellationToken::new(),
            },
            None,
        )
        .unwrap();
        assert!(matches!(
            wire::control_transport::exchange(f.gateway().as_ref(), other, &original).await,
            Err(IntegrationError::Denied)
        ));
    }
    // Gateway denials are encoded Replies. A transport-level Denied here proves
    // neither management nor connect dispatch was entered.
    assert!(matches!(
        wire::control_transport::exchange(f.gateway().as_ref(), local, &request.encode().unwrap())
            .await,
        Err(IntegrationError::Denied)
    ));
}
fn static_input(secret: &str) -> wire::StaticInput {
    wire::StaticInput {
        key: AccountKey::parse("wire-static").unwrap(),
        app: AppId::parse("static-bearer").unwrap(),
        metadata: AccountMetadata::new("Private wire account".into(), None).unwrap(),
        value: wire::SecretSubmission::new(SecretString::from(secret.to_owned())).unwrap(),
    }
}
fn account(reply: Reply) -> ConnectedAccount {
    let Reply::Account { account } = reply else {
        panic!("account reply");
    };
    *account
}

#[tokio::test]
async fn integration_service_contract_control_crud_preserves_owner_stale_versions_and_redaction() {
    let f = Box::pin(Fixture::new()).await;
    assert!(
        matches!(command(&f,&f.owner,Command::BindingApps {}).await,Reply::BindingApps {apps} if !apps.is_empty())
    );
    let a = account(
        command(
            &f,
            &f.owner,
            Command::CreateStatic {
                input: static_input("wire-static-secret"),
            },
        )
        .await,
    );
    for cmd in [
        Command::GetAccount { account: a.id },
        Command::ListAccounts {
            page: wire::Page {
                after: None,
                size: 10,
            },
        },
        Command::AccountChoices {
            app: AppId::parse("static-bearer").unwrap(),
            page: wire::Page {
                after: None,
                size: 10,
            },
        },
        Command::InspectGrants {
            account: a.id,
            after: None,
        },
    ] {
        assert!(!matches!(
            command(&f, &f.owner, cmd).await,
            Reply::Failure { .. }
        ));
    }
    let mut foreign = f.owner.owner().clone();
    foreign.subject = OwnerSubject::parse(Uuid::new_v4().to_string()).unwrap();
    let foreign = VerifiedOwner::from_verified_client(foreign);
    assert!(matches!(
        command(&f, &foreign, Command::GetAccount { account: a.id }).await,
        Reply::Failure {
            failure: wire::Failure::NotFound
        }
    ));
    let b = account(
        command(
            &f,
            &f.owner,
            Command::ReplaceStatic {
                account: a.id,
                expected: a.revision,
                input: static_input("wire-static-replacement"),
            },
        )
        .await,
    );
    assert!(matches!(
        command(
            &f,
            &f.owner,
            Command::ReplaceStatic {
                account: a.id,
                expected: a.revision,
                input: static_input("wire-static-secret")
            }
        )
        .await,
        Reply::Failure {
            failure: wire::Failure::Conflict
        }
    ));
    let c = account(
        command(
            &f,
            &f.owner,
            Command::Disconnect {
                account: b.id,
                expected: b.authorization_revision,
            },
        )
        .await,
    );
    assert!(matches!(
        command(
            &f,
            &f.owner,
            Command::DeleteAccount {
                account: c.id,
                expected: c.authorization_revision
            }
        )
        .await,
        Reply::Done {}
    ));
    assert!(matches!(
        command(&f, &f.owner, Command::GetAccount { account: c.id }).await,
        Reply::Failure {
            failure: wire::Failure::NotFound
        }
    ));
}
async fn begin(f: &Fixture) -> (ConnectAttempt, String) {
    let cmd = super::command(None);
    let reply = command(
        f,
        &f.owner,
        Command::BeginConnect {
            app: cmd.app,
            config: cmd.config,
            capabilities: cmd.requested_capabilities,
            return_target: cmd.return_target,
            reconnect: None,
        },
    )
    .await;
    let Reply::ConnectStarted {
        attempt,
        authorization_url,
    } = reply
    else {
        panic!("connect start");
    };
    let url = url::Url::parse(authorization_url.as_str()).unwrap();
    let state = url
        .query_pairs()
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .into_owned();
    (attempt, state)
}
#[tokio::test]
async fn integration_service_contract_control_connect_uses_original_browser_and_private_code_exchange()
 {
    let f = Box::pin(Fixture::new()).await;
    assert!(
        matches!(command(&f,&f.owner,Command::ConnectableApps {}).await,Reply::ConnectableApps {apps} if !apps.is_empty())
    );
    let (a, state) = Box::pin(begin(&f)).await;
    assert!(
        matches!(command(&f,&f.owner,Command::GetConnectAttempt {attempt:a.id}).await,Reply::ConnectAttempt {attempt} if attempt.status==ConnectStatus::Pending)
    );
    let reply = command(
        &f,
        &f.owner,
        Command::FinishConnect {
            provider: ProviderId::parse("google").unwrap(),
            state: wire::SecretSubmission::new(state.into()).unwrap(),
            code: Some(wire::SecretSubmission::new("code-sentinel".into()).unwrap()),
            error: None,
        },
    )
    .await;
    assert!(
        matches!(reply,Reply::ConnectAttempt {attempt} if attempt.status==ConnectStatus::Connected)
    );
    assert_eq!(f.script.calls.load(Ordering::SeqCst), 1);
    let (cancelled, _) = Box::pin(begin(&f)).await;
    assert!(
        matches!(command(&f,&f.owner,Command::CancelConnect {attempt:cancelled.id}).await,Reply::ConnectAttempt {attempt} if attempt.status==ConnectStatus::Cancelled)
    );
}
