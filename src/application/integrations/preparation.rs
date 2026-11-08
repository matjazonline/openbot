use super::renewal::ExecutionCredentials;
use super::*;
use crate::application::services::mcp_client::{
    McpPreparation, McpPreparationFailure as Failure, McpPreparationResult, McpRequestAuthority,
};
use crate::domain::integrations::{CredentialGeneration, IntegrationError};
use async_trait::async_trait;
use std::sync::Arc;

enum Subject {
    Business(Box<BusinessSubject>),
    Configuration(Box<OwnerDiscoveryContext>),
}
struct BusinessSubject {
    invocation: AuthorizedInvocation,
    context: ExecutionContext,
}
struct Authority {
    credentials: Arc<dyn ExecutionCredentials>,
    subject: Subject,
    generation: CredentialGeneration,
}

pub(super) fn business(
    credentials: Arc<dyn ExecutionCredentials>,
    i: &AuthorizedInvocation,
    c: &ExecutionContext,
    generation: CredentialGeneration,
) -> McpPreparation {
    McpPreparation::new(
        i.deadline,
        c.cancellation.clone(),
        Arc::new(Authority {
            credentials,
            subject: Subject::Business(Box::new(BusinessSubject {
                invocation: i.clone(),
                context: c.clone(),
            })),
            generation,
        }),
    )
}
pub(super) fn configuration(
    credentials: Arc<dyn ExecutionCredentials>,
    c: &OwnerDiscoveryContext,
    generation: CredentialGeneration,
) -> McpPreparation {
    McpPreparation::new(
        c.deadline,
        c.cancellation.clone(),
        Arc::new(Authority {
            credentials,
            subject: Subject::Configuration(Box::new(c.clone())),
            generation,
        }),
    )
}
#[async_trait]
impl McpRequestAuthority for Authority {
    async fn check(&self) -> McpPreparationResult<()> {
        let result = match &self.subject {
            Subject::Business(subject) => {
                self.credentials
                    .check_generation(&subject.invocation, &subject.context, self.generation)
                    .await
            }
            Subject::Configuration(c) => {
                self.credentials
                    .check_configuration(c, self.generation)
                    .await
            }
        };
        result.map_err(|error| match error {
            IntegrationError::Conflict => Failure::Changed,
            IntegrationError::Unavailable | IntegrationError::Storage => Failure::Unavailable,
            _ => Failure::Denied,
        })
    }
}
