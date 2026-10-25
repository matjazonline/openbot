//! The production resource authority of action reconciliation (P08-D3 §D3.1): the command
//! actor's current access to the action's own resource, decided by the frozen target's resource
//! kind in either target form, the way dispatch reads it.
use super::*;
use crate::adapters::persistence::workflow::message::{MESSAGING, MessageActionAuthority};

/// Which authority a frozen target answers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResourceRoute {
    /// The company's built-in messaging resource.
    Messaging,
    /// A company HTTP or MCP connection.
    Connection(Uuid),
}

/// HTTP is `Connection` when published with a slot and `Local` without one; MCP is always
/// `Connection`; messaging is `Local`. Provisioning is never reconcilable (P08-D2 §D2.2a item 4:
/// its markers always carry receipts) and fails closed, as does every other kind.
fn resource_route(target: &ActionTarget) -> AppResult<ResourceRoute> {
    let (resource, kind) = match target {
        ActionTarget::Connection {
            resource,
            resource_kind,
        }
        | ActionTarget::Local {
            resource,
            resource_kind,
        } => (*resource, resource_kind.as_str()),
    };
    match (target, kind) {
        (ActionTarget::Local { .. }, MESSAGING) => Ok(ResourceRoute::Messaging),
        (_, "http") | (ActionTarget::Connection { .. }, "mcp") => {
            Ok(ResourceRoute::Connection(resource))
        }
        _ => Err(missing()),
    }
}

/// The production [`SqlReconciliationResources`]: no test double stands in for it.
#[derive(Debug, Clone, Copy, Default)]
pub struct CompanyReconciliationResources;

#[async_trait]
impl SqlReconciliationResources for CompanyReconciliationResources {
    async fn lock_resource(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: WorkflowActor,
        frozen: &ActionRunAuthority,
    ) -> AppResult<ResourceStatus> {
        match resource_route(&frozen.action.request().target)? {
            ResourceRoute::Messaging => {
                MessageActionAuthority
                    .lock_resource(tx, actor, frozen)
                    .await
            }
            ResourceRoute::Connection(resource) => {
                let company = frozen.action.scope().company;
                // Company before the connection, the order dispatch and connection edits take.
                authority::authorize_company(tx, company, actor).await?;
                super::super::resources::inspect_on(tx, company, RuntimeResourceId::new(resource))
                    .await?
                    .ok_or_else(missing)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(local: bool, kind: &str) -> ActionTarget {
        let (resource, resource_kind) = (Uuid::from_u128(7), TypeName::parse(kind).unwrap());
        if local {
            ActionTarget::Local {
                resource,
                resource_kind,
            }
        } else {
            ActionTarget::Connection {
                resource,
                resource_kind,
            }
        }
    }

    /// F065: the five forms, decided by kind in either form; everything else fails closed.
    #[test]
    fn reconciliation_resources_route_by_kind_in_either_form() {
        let connection = Some(ResourceRoute::Connection(Uuid::from_u128(7)));
        assert_eq!(resource_route(&target(false, "http")).ok(), connection);
        assert_eq!(resource_route(&target(true, "http")).ok(), connection);
        assert_eq!(resource_route(&target(false, "mcp")).ok(), connection);
        assert_eq!(
            resource_route(&target(true, MESSAGING)).ok(),
            Some(ResourceRoute::Messaging)
        );
        for (local, kind) in [
            (
                true,
                crate::application::workflow::provisioning::PROVISIONING,
            ),
            (
                false,
                crate::application::workflow::provisioning::PROVISIONING,
            ),
            (true, "mcp"),
            (false, MESSAGING),
            (true, "fixture"),
            (false, "smtp"),
        ] {
            assert!(
                matches!(
                    resource_route(&target(local, kind)),
                    Err(AppError::NotFound(_))
                ),
                "{kind} (local: {local}) must fail closed"
            );
        }
    }
}
