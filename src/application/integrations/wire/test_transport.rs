//! In-memory serialization transport. Live capabilities stay in a required local factory;
//! this is deliberately not a production remote gateway or an authentication protocol.
use super::*;
use crate::application::integrations::*;
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::sync::Arc;

#[derive(Clone, Copy)]
pub(crate) enum Fault {
    None,
    UnknownVersion,
    ForeignOwner,
    CancelBeforeDispatch,
    LostReply,
    InvalidReplyVersion,
}
pub(crate) struct SerializationGateway {
    inner: Arc<dyn ExecutionGateway>,
    owner: AccountOwner,
    fault: Fault,
}
impl SerializationGateway {
    pub fn new(inner: Arc<dyn ExecutionGateway>, owner: AccountOwner, fault: Fault) -> Self {
        Self {
            inner,
            owner,
            fault,
        }
    }
    fn round_trip(&self, request: &Request) -> IntegrationResult<Request> {
        let mut value: serde_json::Value =
            serde_json::from_slice(&request.encode()?).map_err(|_| IntegrationError::Invalid)?;
        match self.fault {
            Fault::UnknownVersion => value["version"] = serde_json::json!(65535),
            Fault::ForeignOwner => value["owner"]["subject"] = serde_json::json!("foreign-claim"),
            _ => {}
        }
        Request::decode(&serde_json::to_vec(&value).map_err(|_| IntegrationError::Invalid)?)
    }
    async fn business(
        &self,
        invocation: AuthorizedInvocation,
        context: ExecutionContext,
        mcp: bool,
    ) -> IntegrationResult<ExecutionOutcome> {
        let request = Request {
            version: PROTOCOL_VERSION,
            identity: invocation.dispatch,
            owner: self.owner.clone(),
            deadline: invocation.deadline,
            cancellation: invocation.cancellation.clone(),
            command: if mcp {
                Command::CallMcp {
                    invocation: Box::new(invocation),
                }
            } else {
                Command::ExecuteHttp {
                    invocation: Box::new(invocation),
                }
            },
        };
        let factory = LocalContextFactory::new(&request, context)?;
        let decoded = match self.round_trip(&request) {
            Ok(decoded) => decoded,
            Err(_) => {
                return Ok(ExecutionOutcome::PreDispatch {
                    failure: PreDispatchFailure::Invalid,
                });
            }
        };
        let context = match factory.resolve(&decoded) {
            Ok(context) => context,
            Err(_) => {
                return Ok(ExecutionOutcome::PreDispatch {
                    failure: PreDispatchFailure::Denied,
                });
            }
        };
        if matches!(self.fault, Fault::CancelBeforeDispatch) {
            context.cancellation.cancel();
        }
        let outcome = match decoded.command {
            Command::ExecuteHttp { invocation } => {
                self.inner.execute_http(*invocation, context).await?
            }
            Command::CallMcp { invocation } => self.inner.call_mcp(*invocation, context).await?,
            _ => return Err(IntegrationError::Denied),
        };
        let response = Response {
            version: PROTOCOL_VERSION,
            identity: decoded.identity,
            reply: Reply::Execution { outcome },
        };
        self.business_reply(response)
    }
    fn business_reply(&self, response: Response) -> IntegrationResult<ExecutionOutcome> {
        if matches!(self.fault, Fault::LostReply) {
            return Ok(ExecutionOutcome::Uncertain);
        }
        let decoded = response.encode().and_then(|mut bytes| {
            if matches!(self.fault, Fault::InvalidReplyVersion) {
                let mut value: serde_json::Value =
                    serde_json::from_slice(&bytes).map_err(|_| IntegrationError::Invalid)?;
                value["version"] = serde_json::json!(65535);
                bytes = serde_json::to_vec(&value).map_err(|_| IntegrationError::Invalid)?;
            }
            Response::decode(&bytes, response.identity)
        });
        // No response after a business invocation proves that it was not sent. The native
        // host's retained observation/receipt remains authoritative, including on lost replies.
        match decoded {
            Ok(Response {
                reply: Reply::Execution { outcome },
                ..
            }) => Ok(outcome),
            _ => Ok(ExecutionOutcome::Uncertain),
        }
    }
}

/// Created from the authenticated LOCAL caller's exact request and its real runtime context.
/// Payload claims cannot mint authority or replace the cancellation token.
struct LocalContextFactory {
    owner: AccountOwner,
    digest: [u8; 32],
    context: ExecutionContext,
}
impl LocalContextFactory {
    fn new(request: &Request, context: ExecutionContext) -> IntegrationResult<Self> {
        Ok(Self {
            owner: request.owner.clone(),
            digest: Sha256::digest(request.encode()?).into(),
            context,
        })
    }
    fn resolve(&self, request: &Request) -> IntegrationResult<ExecutionContext> {
        if request.owner != self.owner
            || <[u8; 32]>::from(Sha256::digest(request.encode()?)) != self.digest
        {
            return Err(IntegrationError::Denied);
        }
        Ok(self.context.clone())
    }
}

#[async_trait]
impl ExecutionGateway for SerializationGateway {
    async fn execute_http(
        &self,
        i: AuthorizedInvocation,
        c: ExecutionContext,
    ) -> IntegrationResult<ExecutionOutcome> {
        Box::pin(self.business(i, c, false)).await
    }
    async fn call_mcp(
        &self,
        i: AuthorizedInvocation,
        c: ExecutionContext,
    ) -> IntegrationResult<ExecutionOutcome> {
        Box::pin(self.business(i, c, true)).await
    }
    async fn discover_mcp(&self, c: OwnerDiscoveryContext) -> IntegrationResult<DiscoveredCatalog> {
        let identity = DispatchId::new(uuid::Uuid::new_v4());
        let request = Request {
            version: PROTOCOL_VERSION,
            identity,
            owner: self.owner.clone(),
            deadline: c.deadline,
            cancellation: CancellationId::parse(identity.as_uuid().to_string())?,
            command: Command::DiscoverMcp {
                authorization: Box::new(c.authorization.clone()),
                host_authority: HostAuthorityReference::parse(identity.as_uuid().to_string())?,
            },
        };
        let encoded = request.encode()?;
        let decoded = self.round_trip(&request)?;
        // The owner proof and configuration authority stay local and are never recreated
        // from these claims. Compare the entire registered projection before using them.
        if decoded.encode()? != encoded || decoded.owner != *c.owner.owner() {
            return Err(IntegrationError::Denied);
        }
        if matches!(self.fault, Fault::CancelBeforeDispatch) {
            c.cancellation.cancel();
        }
        let catalog = self.inner.discover_mcp(c).await?;
        let response = Response {
            version: PROTOCOL_VERSION,
            identity,
            reply: Reply::Catalog { catalog },
        };
        if matches!(self.fault, Fault::LostReply | Fault::InvalidReplyVersion) {
            return Err(IntegrationError::Unavailable);
        }
        match Response::decode(&response.encode()?, identity)?.reply {
            Reply::Catalog { catalog } => Ok(catalog),
            _ => Err(IntegrationError::Invalid),
        }
    }
}
