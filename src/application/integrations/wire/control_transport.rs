//! In-memory control conformance adapter. Owner/browser/work/consent capabilities
//! arrive through trusted local arguments; none are created by decoding bytes.
use super::*;
use crate::application::integrations::*;
use secrecy::ExposeSecret;
use sha2::{Digest, Sha256};

/// Constructed from the original local request before transport. Decoding a frame
/// cannot create the owner, token, consent, or its association with that request.
pub(crate) struct LocalContext<'a> {
    owner: &'a VerifiedOwner,
    work: ConnectWork,
    confirmation: Option<OwnerConfirmation>,
    identity: DispatchId,
    cancellation: CancellationId,
    digest: [u8; 32],
}
impl<'a> LocalContext<'a> {
    pub(crate) fn new(
        request: &Request,
        owner: &'a VerifiedOwner,
        work: ConnectWork,
        confirmation: Option<OwnerConfirmation>,
    ) -> IntegrationResult<Self> {
        Ok(Self {
            owner,
            work,
            confirmation,
            identity: request.identity,
            cancellation: request.cancellation.clone(),
            digest: Sha256::digest(request.encode()?).into(),
        })
    }
}

pub(crate) async fn exchange(
    gateway: &dyn IntegrationGateway,
    local: LocalContext<'_>,
    bytes: &[u8],
) -> IntegrationResult<Vec<u8>> {
    let request = Request::decode(bytes)?;
    if local.identity != request.identity
        || local.cancellation != request.cancellation
        || local.digest != <[u8; 32]>::from(Sha256::digest(bytes))
        || local.owner.owner() != &request.owner
        || request.deadline != local.work.deadline
        || Utc::now() >= local.work.deadline
        || local.work.cancellation.is_cancelled()
    {
        return Err(IntegrationError::Denied);
    }
    let reply = match control(
        gateway,
        local.owner,
        request.command,
        local.work,
        local.confirmation,
    )
    .await
    {
        Ok(reply) => reply,
        Err(error) => Reply::Failure {
            failure: error.into(),
        },
    };
    Response {
        version: PROTOCOL_VERSION,
        identity: request.identity,
        reply,
    }
    .encode()
}
fn input(input: StaticInput) -> StaticAccountInput {
    StaticAccountInput {
        key: input.key,
        app: input.app,
        metadata: input.metadata,
        value: input.value.into_secret(),
    }
}
async fn control(
    gateway: &dyn IntegrationGateway,
    owner: &VerifiedOwner,
    command: Command,
    work: ConnectWork,
    confirmation: Option<OwnerConfirmation>,
) -> IntegrationResult<Reply> {
    match command {
        Command::BindingApps {} => Ok(Reply::BindingApps {
            apps: gateway.binding_apps().await?,
        }),
        Command::AccountChoices { app, page } => Ok(Reply::Accounts {
            page: gateway.account_choices(owner, &app, page.local()?).await?,
        }),
        Command::ListAccounts { page } => Ok(Reply::Accounts {
            page: gateway.list_accounts(owner, page.local()?).await?,
        }),
        Command::GetAccount { account } => Ok(Reply::Account {
            account: Box::new(gateway.get_account(owner, account).await?),
        }),
        Command::CreateStatic { input: submission } => Ok(Reply::Account {
            account: Box::new(
                gateway
                    .create_static_account(owner, input(submission))
                    .await?,
            ),
        }),
        Command::ReplaceStatic {
            account,
            expected,
            input: submission,
        } => Ok(Reply::Account {
            account: Box::new(
                gateway
                    .replace_static_account(owner, account, expected, input(submission))
                    .await?,
            ),
        }),
        Command::Disconnect { account, expected } => Ok(Reply::Account {
            account: Box::new(gateway.disconnect_account(owner, account, expected).await?),
        }),
        Command::DeleteAccount { account, expected } => {
            gateway.delete_account(owner, account, expected).await?;
            Ok(Reply::Done {})
        }
        Command::InspectGrants { account, after } => Ok(Reply::Grants {
            inspection: gateway.inspect_grants(owner, account, after).await?,
        }),
        Command::RevokeGrant { grant, expected } => {
            gateway.revoke_use_grant(owner, grant, expected).await?;
            Ok(Reply::Done {})
        }
        command => connect_or_grant(gateway, owner, command, work, confirmation).await,
    }
}
async fn connect_or_grant(
    gateway: &dyn IntegrationGateway,
    owner: &VerifiedOwner,
    command: Command,
    work: ConnectWork,
    confirmation: Option<OwnerConfirmation>,
) -> IntegrationResult<Reply> {
    match command {
        Command::ConnectableApps {} => Ok(Reply::ConnectableApps {
            apps: gateway.connectable_apps().await?,
        }),
        Command::BeginConnect {
            app,
            config,
            capabilities,
            return_target,
            reconnect,
        } => {
            let start = gateway
                .begin_connect(
                    owner,
                    BeginConnect {
                        app,
                        config,
                        requested_capabilities: capabilities,
                        return_target,
                        reconnect,
                    },
                )
                .await?;
            Ok(Reply::ConnectStarted {
                attempt: start.attempt,
                authorization_url: AuthorizationUrlData::try_from(
                    start.authorization_url.as_str().to_owned(),
                )?,
            })
        }
        Command::FinishConnect {
            provider,
            state,
            code,
            error,
        } => {
            let callback = ConnectCallback {
                provider,
                state: state.into_secret().expose_secret().to_owned(),
                code: code.map(SecretSubmission::into_secret),
                error: error.map(|e| e.as_str().to_owned()),
            };
            Ok(Reply::ConnectAttempt {
                attempt: gateway.finish_connect(owner, callback, work).await?,
            })
        }
        Command::CancelConnect { attempt } => Ok(Reply::ConnectAttempt {
            attempt: gateway.cancel_connect(owner, attempt).await?,
        }),
        Command::GetConnectAttempt { attempt } => Ok(Reply::ConnectAttempt {
            attempt: gateway.get_connect_attempt(owner, attempt).await?,
        }),
        command @ Command::CreateGrant { .. } => {
            let confirmation = confirmation.ok_or(IntegrationError::Denied)?;
            check_confirmation(owner, &command, &confirmation)?;
            Ok(Reply::Grant {
                grant: gateway.create_use_grant(confirmation).await?,
            })
        }
        // Execution/discovery have their own required real-context factory in test_transport.
        _ => Err(IntegrationError::Invalid),
    }
}
fn check_confirmation(
    owner: &VerifiedOwner,
    command: &Command,
    c: &OwnerConfirmation,
) -> IntegrationResult<()> {
    let Command::CreateGrant {
        account,
        expected,
        scope,
        binding,
        app,
        destination,
        purpose,
        host_authority,
    } = command
    else {
        return Err(IntegrationError::Invalid);
    };
    if c.owner.owner() != owner.owner()
        || &c.account != account
        || &c.expected_authorization != expected
        || &c.host_scope != scope
        || &c.binding != binding
        || &c.app != app
        || &c.destination != destination
        || &c.purpose != purpose
        || &c.host_authority != host_authority
    {
        return Err(IntegrationError::Denied);
    }
    Ok(())
}
