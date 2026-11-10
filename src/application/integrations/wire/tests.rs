use super::*;
use crate::application::integrations::wire as protocol;
use protocol::{Command, PROTOCOL_VERSION, Reply, Request, Response};

fn invocation() -> AuthorizedInvocation {
    let account = account();
    AuthorizedInvocation {
        authorization: snapshot(&grant(&account)),
        operation: OperationId::parse("messages.read").unwrap(),
        target: OperationTarget::Http,
        arguments: OperationArguments::new(
            json!({"method":"GET","path":"/gmail/v1/users/me/threads/t"}),
        )
        .unwrap(),
        request_digest: [0; 32],
        correlation: CorrelationId::parse("correlation").unwrap(),
        dispatch: DispatchId::new(Uuid::new_v4()),
        deadline: Utc::now() + chrono::Duration::seconds(30),
        cancellation: CancellationId::parse("local-cancellation-identity").unwrap(),
        host_authority: HostAuthorityReference::parse("opaque-live-reference").unwrap(),
    }
}
fn request(command: Command) -> Request {
    let i = invocation();
    Request {
        version: PROTOCOL_VERSION,
        identity: i.dispatch,
        owner: account().owner,
        deadline: i.deadline,
        cancellation: i.cancellation,
        command,
    }
}
fn business() -> Request {
    let i = invocation();
    Request {
        version: PROTOCOL_VERSION,
        identity: i.dispatch,
        owner: account().owner,
        deadline: i.deadline,
        cancellation: i.cancellation.clone(),
        command: Command::ExecuteHttp {
            invocation: Box::new(i),
        },
    }
}
fn submission() -> protocol::SecretSubmission {
    protocol::SecretSubmission::new(SecretString::from("private-command-submission-sentinel"))
        .unwrap()
}
fn static_input() -> protocol::StaticInput {
    protocol::StaticInput {
        key: AccountKey::parse("private-key").unwrap(),
        app: AppId::parse("static-api-key").unwrap(),
        metadata: AccountMetadata::new("Private account".into(), None).unwrap(),
        value: submission(),
    }
}

#[test]
fn integration_service_contract_unknown_protocol_fields_depth_and_bounds_fail_closed() {
    let original = serde_json::to_value(business()).unwrap();
    for changed in [
        json!({"version":2,"command":{"command":"future"}}),
        json!({"version":1,"command":{"command":"future"}}),
        {
            let mut v = original.clone();
            v["lease"] = json!("host-lease-must-not-travel");
            v
        },
        {
            let mut v = original.clone();
            v["command"]["extra"] = json!(true);
            v
        },
        {
            let mut v = original.clone();
            v["command"]["invocation"]["arguments"] = json!({"authorization":"override"});
            v
        },
    ] {
        assert!(Request::decode(&serde_json::to_vec(&changed).unwrap()).is_err());
    }
    assert!(matches!(
        Request::decode(&vec![b' '; protocol::MAX_FRAME_BYTES + 1]),
        Err(IntegrationError::Limit)
    ));
    let nested = format!("{}0{}", "[".repeat(256), "]".repeat(256));
    let mut encoded = String::from_utf8(business().encode().unwrap()).unwrap();
    let old_arguments =
        serde_json::to_string(&original["command"]["invocation"]["arguments"]).unwrap();
    let key = format!("\"arguments\":{old_arguments}");
    assert!(encoded.contains(&key));
    encoded = encoded.replace(&key, &format!("\"arguments\":{{\"nested\":{nested}}}"));
    assert!(Request::decode(encoded.as_bytes()).is_err());
    let mut unit = serde_json::to_value(request(Command::BindingApps {})).unwrap();
    unit["command"]["unrecognized"] = json!(true);
    assert!(Request::decode(&serde_json::to_vec(&unit).unwrap()).is_err());
    assert!(Request::decode(br#"{"version":2,"version":1}"#).is_err());
    assert!(
        request(Command::ListAccounts {
            page: protocol::Page {
                after: None,
                size: 0
            }
        })
        .encode()
        .is_err()
    );
}

#[test]
fn integration_service_contract_execution_identity_is_bound_to_the_local_context_reference() {
    let original = serde_json::to_value(business()).unwrap();
    for field in ["identity", "deadline", "cancellation", "owner"] {
        let mut changed = original.clone();
        changed[field] = match field {
            "identity" => json!(Uuid::new_v4()),
            "deadline" => json!(Utc::now() - chrono::Duration::seconds(1)),
            "cancellation" => json!("different-runtime-context"),
            _ => json!(owner("foreign-client", "owner")),
        };
        assert!(
            Request::decode(&serde_json::to_vec(&changed).unwrap()).is_err(),
            "{field}"
        );
    }
    let decoded = Request::decode(&business().encode().unwrap()).unwrap();
    let bytes = decoded.encode().unwrap();
    for forbidden in [
        "CancellationToken",
        "WorkflowLease",
        "sqlx",
        "access_token",
        "refresh_token",
    ] {
        assert!(!String::from_utf8_lossy(&bytes).contains(forbidden));
    }
}

fn management_commands() -> Vec<Command> {
    let a = account();
    let g = grant(&a);
    vec![
        Command::BindingApps {},
        Command::AccountChoices {
            app: g.app.clone(),
            page: protocol::Page {
                after: None,
                size: 1,
            },
        },
        Command::ListAccounts {
            page: protocol::Page {
                after: Some(a.id),
                size: MAX_PAGE_SIZE,
            },
        },
        Command::GetAccount { account: a.id },
        Command::CreateStatic {
            input: static_input(),
        },
        Command::ReplaceStatic {
            account: a.id,
            expected: a.revision,
            input: static_input(),
        },
        Command::Disconnect {
            account: a.id,
            expected: a.authorization_revision,
        },
        Command::DeleteAccount {
            account: a.id,
            expected: a.authorization_revision,
        },
        Command::InspectGrants {
            account: a.id,
            after: Some(g.id),
        },
    ]
}

fn connect_commands() -> Vec<Command> {
    let a = account();
    let g = grant(&a);
    let attempt = ConnectAttemptId::new(Uuid::new_v4());
    vec![
        Command::ConnectableApps {},
        Command::BeginConnect {
            app: g.app.clone(),
            config: a.auth_config_id.clone(),
            capabilities: RequestedCapabilities::try_from(vec![
                Capability::parse("gmail.read").unwrap(),
            ])
            .unwrap(),
            return_target: SafeReturnTarget::parse("/ui/profile/vault".into()).unwrap(),
            reconnect: Some(ReconnectTarget {
                account: a.id,
                authorization: a.authorization_revision,
            }),
        },
        Command::FinishConnect {
            provider: a.provider_id.clone(),
            state: submission(),
            code: Some(submission()),
            error: None,
        },
        Command::CancelConnect { attempt },
        Command::GetConnectAttempt { attempt },
    ]
}

fn grant_commands() -> Vec<Command> {
    let a = account();
    let g = grant(&a);
    vec![
        Command::CreateGrant {
            account: a.id,
            expected: a.authorization_revision,
            scope: g.host_scope.clone(),
            binding: g.binding.clone(),
            app: g.app.clone(),
            destination: g.destination.clone(),
            purpose: g.purpose,
            host_authority: HostAuthorityReference::parse("opaque-confirmation").unwrap(),
        },
        Command::RevokeGrant {
            grant: g.id,
            expected: g.revision,
        },
        Command::DiscoverMcp {
            authorization: Box::new(snapshot(&g)),
            host_authority: HostAuthorityReference::parse("opaque-configuration").unwrap(),
        },
    ]
}

#[test]
fn integration_service_contract_metadata_and_sensitive_submission_commands_round_trip() {
    let commands = management_commands()
        .into_iter()
        .chain(connect_commands())
        .chain(grant_commands());
    for command in commands {
        let encoded = request(command).encode().unwrap();
        assert_eq!(
            Request::decode(&encoded).unwrap().encode().unwrap(),
            encoded
        );
    }
    assert!(!format!("{:?}", submission()).contains("submission-sentinel"));
}

#[test]
fn integration_service_contract_outcomes_are_exclusive_bounded_and_identity_checked() {
    for outcome in [
        ExecutionOutcome::PreDispatch {
            failure: PreDispatchFailure::Denied,
        },
        ExecutionOutcome::Committed {
            data: ProviderData::from_redacted(json!({"message":"provider-id","thread":"t"}))
                .unwrap(),
        },
        ExecutionOutcome::Uncertain,
    ] {
        let response = Response {
            version: PROTOCOL_VERSION,
            identity: DispatchId::new(Uuid::new_v4()),
            reply: Reply::Execution { outcome },
        };
        let bytes = response.encode().unwrap();
        assert_eq!(
            Response::decode(&bytes, response.identity)
                .unwrap()
                .encode()
                .unwrap(),
            bytes
        );
        assert!(Response::decode(&bytes, DispatchId::new(Uuid::new_v4())).is_err());
        let mut changed = serde_json::to_value(response).unwrap();
        changed["reply"]["outcome"]["failure"] = json!("retry");
        changed["reply"]["outcome"]["data"] = json!({"unsafe":"conflicting-outcome"});
        assert!(
            Response::decode(
                &serde_json::to_vec(&changed).unwrap(),
                serde_json::from_value(changed["identity"].clone()).unwrap()
            )
            .is_err()
        );
    }
    assert!(
        serde_json::from_value::<ProviderData>(json!({"too_large":"x".repeat(65536)})).is_err()
    );
}

#[test]
fn integration_service_contract_large_local_catalog_has_explicit_transport_overflow() {
    use crate::entities::mcp::{McpDiscoveredTool, McpToolName};
    let tools = (0..40)
        .map(|n| McpDiscoveredTool {
            name: McpToolName::try_from(format!("tool_{n}")).unwrap(),
            description: "x".repeat(4096),
            input_schema: json!({"type":"object"}),
            output_schema: None,
        })
        .collect();
    let catalog = DiscoveredCatalog::from_redacted(tools).unwrap();
    let response = Response {
        version: PROTOCOL_VERSION,
        identity: DispatchId::new(Uuid::new_v4()),
        reply: Reply::Catalog { catalog },
    };
    assert!(matches!(response.encode(), Err(IntegrationError::Limit)));
}

#[test]
fn integration_service_contract_public_metadata_replies_round_trip_without_private_fields() {
    let a = account();
    let g = grant(&a);
    let attempt = ConnectAttempt {
        return_target: SafeReturnTarget::parse("/ui/profile/vault".into()).unwrap(),
        id: ConnectAttemptId::new(Uuid::new_v4()),
        status: ConnectStatus::Connected,
        account: Some(a.id),
    };
    let replies = vec![
        Reply::BindingApps {
            apps: static_registry().binding_apps(),
        },
        Reply::Accounts {
            page: AccountPage {
                accounts: vec![a.clone()],
                next: None,
            },
        },
        Reply::Account {
            account: Box::new(a.clone()),
        },
        Reply::Grants {
            inspection: GrantInspection {
                next: None,
                grants: vec![g.clone()],
                attachment_available: true,
            },
        },
        Reply::Grant { grant: g.clone() },
        Reply::ConnectableApps {
            apps: vec![ConnectApp {
                provider: a.provider_id.clone(),
                id: g.app,
                config: a.auth_config_id,
                capabilities: vec![Capability::parse("gmail.read").unwrap()],
            }],
        },
        Reply::ConnectAttempt {
            attempt: attempt.clone(),
        },
        Reply::ConnectStarted {
            attempt,
            authorization_url: protocol::AuthorizationUrlData::try_from(
                "https://accounts.google.com/o/oauth2/v2/auth?state=opaque&client_id=public"
                    .to_owned(),
            )
            .unwrap(),
        },
        Reply::Catalog {
            catalog: DiscoveredCatalog::from_redacted(vec![]).unwrap(),
        },
        Reply::Done {},
        Reply::Failure {
            failure: protocol::Failure::Credential,
        },
    ];
    for reply in replies {
        let response = Response {
            version: PROTOCOL_VERSION,
            identity: DispatchId::new(Uuid::new_v4()),
            reply,
        };
        let bytes = response.encode().unwrap();
        assert_eq!(
            Response::decode(&bytes, response.identity)
                .unwrap()
                .encode()
                .unwrap(),
            bytes
        );
        for private in [
            "access_token",
            "refresh_token",
            "ciphertext",
            "TypedInjection",
            "ResolvedCredential",
        ] {
            assert!(!String::from_utf8_lossy(&bytes).contains(private));
        }
    }
    assert!(
        protocol::AuthorizationUrlData::try_from(
            "https://accounts.google.com/auth?client_secret=sentinel".to_owned()
        )
        .is_err()
    );
}
