use super::*;
use crate::{
    application::integrations::choices::BindingStatusRequest,
    domain::integrations::authority::CredentialSource, entities::mcp::McpAuth,
};
struct SourceRow {
    destination: ApprovedDestination,
    revision: i64,
    reference: Option<IntegrationReference>,
}
async fn source_on(
    tx: &mut Transaction<'_, Postgres>,
    target: BindingTarget,
) -> AppResult<SourceRow> {
    match target.kind {
        BindingKind::Http => {
            let c = crate::adapters::persistence::workflow::http::connection_on(
                tx,
                CompanyId::new(target.company),
                target.connection,
            )
            .await?
            .ok_or_else(denied)?;
            Ok(SourceRow {
                destination: ApprovedDestination::parse(c.origin.as_str())?,
                revision: c.revision,
                reference: match c.credential_source {
                    CredentialSource::Integration(reference) => Some(reference),
                    _ => None,
                },
            })
        }
        BindingKind::Mcp => {
            let c = crate::adapters::persistence::mcp::connection_on(
                tx,
                target.company,
                target.connection,
            )
            .await?
            .ok_or_else(denied)?;
            Ok(SourceRow {
                destination: ApprovedDestination::parse(c.endpoint.as_str())?,
                revision: c.revision,
                reference: match c.auth {
                    McpAuth::Integration(reference) => Some(reference),
                    _ => None,
                },
            })
        }
    }
}
impl PostgresIntegrationBindings {
    pub(super) async fn inspect_source(
        &self,
        actor: Uuid,
        target: BindingTarget,
    ) -> AppResult<BindingSourceInfo> {
        let mut tx = self.persistence.pool().begin().await?;
        sqlx::query("SET LOCAL lock_timeout='5s'")
            .execute(&mut *tx)
            .await?;
        users_on(&mut tx, vec![actor]).await?;
        crate::adapters::persistence::workflow::authority::authorize_company(
            &mut tx,
            CompanyId::new(target.company),
            WorkflowActor::authenticated(actor)?,
        )
        .await?;
        let current = source_on(&mut tx, target).await?;
        let source = match &current.reference {
            None => CurrentCredentialSource::Owned,
            Some(reference) => CurrentCredentialSource::Vault {
                account: reference.account,
                status: self
                    .vault
                    .binding_status(BindingStatusRequest {
                        reference,
                        client: &client_namespace(),
                        scope: &company_binding(target.company),
                        binding: &binding(target.company, target.connection, target.kind),
                    })
                    .await?,
            },
        };
        tx.rollback().await?;
        Ok(BindingSourceInfo {
            target,
            destination: current.destination,
            revision: current.revision,
            source,
        })
    }
}
