//! Provider installations, their secrets, and the channel bindings that use them.
//!
//! Three ports rather than one, because they have three different blast radii. Reading an
//! installation is an ordinary tenant-scoped list; reading a *credential* must state an exact
//! `(company, installation, transport, kind)` scope and hands back a [`SecretString`] that no
//! broad projection ever sees; and binding lifecycle is an audited mutation whose actor and reason
//! are arguments rather than something an adapter invents.

use crate::application::ports::integration::{
    BindingStatusChange, BindingWrite, ChannelBindingPersistence,
};

use std::sync::Arc;

use async_trait::async_trait;
use secrecy::SecretString;
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        creation::CreationProvenance,
        transport::{
            ChannelBinding, ExternalTenantKey, InstallationId, InstallationStatus,
            IntegrationCredentialKind, IntegrationInstallation, TransportKind,
        },
    },
};

/// Everything installing (or re-installing) one provider account sets.
#[derive(Debug, Clone)]
pub struct InstallationWrite {
    pub company_id: Uuid,
    pub transport: TransportKind,
    pub external_tenant_key: ExternalTenantKey,
    pub display_name: String,
    pub granted_scopes: Vec<String>,
    pub actor: CreationProvenance,
}

/// One audited installation status transition.
#[derive(Debug, Clone)]
pub struct InstallationStatusChange {
    pub company_id: Uuid,
    pub installation_id: InstallationId,
    pub status: InstallationStatus,
    pub actor: CreationProvenance,
}

/// The exact scope one credential read or write is allowed to touch.
///
/// Every field is also part of the envelope's authenticated context, so a scope that does not
/// match the stored row fails to decrypt rather than returning another credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialScope {
    pub company_id: Uuid,
    pub installation_id: InstallationId,
    pub transport: TransportKind,
    pub kind: IntegrationCredentialKind,
}

/// Company-scoped provider accounts. Nothing here returns a token.
#[async_trait]
pub trait InstallationPersistence: Send + Sync {
    /// Install, or refresh an existing installation of the same external account into the same
    /// company. Re-installing an external account that already belongs to *another* company is a
    /// conflict, not an adoption.
    async fn install(&self, write: InstallationWrite) -> AppResult<IntegrationInstallation>;

    async fn get_installation(
        &self,
        company_id: Uuid,
        installation_id: InstallationId,
    ) -> AppResult<Option<IntegrationInstallation>>;

    /// Resolve the provider's own account identifier, for an inbound event that names a workspace
    /// before it names a company.
    async fn find_installation_by_tenant(
        &self,
        transport: TransportKind,
        external_tenant_key: &ExternalTenantKey,
    ) -> AppResult<Option<IntegrationInstallation>>;

    async fn list_installations(&self, company_id: Uuid)
    -> AppResult<Vec<IntegrationInstallation>>;

    async fn set_installation_status(
        &self,
        change: InstallationStatusChange,
    ) -> AppResult<IntegrationInstallation>;
}

/// The only way a provider secret enters or leaves the database.
///
/// Separate from [`InstallationPersistence`] so that a caller holding an installation list has no
/// type-level route to a token at all.
#[async_trait]
pub trait InstallationCredentialStore: Send + Sync {
    async fn store_credential(
        &self,
        scope: &CredentialScope,
        secret: SecretString,
    ) -> AppResult<()>;

    /// `None` means no credential of that kind is stored. A credential that exists but cannot be
    /// authenticated against `scope` is an error, never a `None`: failing open here would turn a
    /// tampered row into "this installation simply has no token".
    async fn read_credential(&self, scope: &CredentialScope) -> AppResult<Option<SecretString>>;

    async fn delete_credential(&self, scope: &CredentialScope) -> AppResult<bool>;
}

/// The rules that hold across an installation and the bindings that depend on it.
#[derive(Clone)]
pub struct IntegrationUseCases {
    installations: Arc<dyn InstallationPersistence>,
    bindings: Arc<dyn ChannelBindingPersistence>,
}

impl IntegrationUseCases {
    pub fn new(
        installations: Arc<dyn InstallationPersistence>,
        bindings: Arc<dyn ChannelBindingPersistence>,
    ) -> Self {
        Self {
            installations,
            bindings,
        }
    }

    /// Link one channel to one protocol endpoint.
    ///
    /// The installed-transport case is checked here as well as in the ingress and delivery
    /// queries: a manager linking a conversation through a lapsed installation should be told so,
    /// rather than getting a binding that quietly carries nothing.
    pub async fn link_binding(&self, write: BindingWrite) -> AppResult<ChannelBinding> {
        write.validate()?;
        if let Some(installation_id) = write.installation_id {
            let installation = self
                .installations
                .get_installation(write.company_id, installation_id)
                .await?
                .ok_or_else(|| {
                    AppError::NotFound("That integration installation was not found.".into())
                })?;
            if !installation.is_usable() {
                return Err(AppError::BadRequest(format!(
                    "The {} installation is '{}' and cannot take new channel links.",
                    installation.transport, installation.status
                )));
            }
        }
        self.bindings.create_binding(write).await
    }

    pub async fn change_binding_status(
        &self,
        change: BindingStatusChange,
    ) -> AppResult<ChannelBinding> {
        change.validate()?;
        self.bindings.set_binding_status(change).await
    }

    pub async fn active_bindings(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
    ) -> AppResult<Vec<ChannelBinding>> {
        self.bindings
            .active_bindings_for_channel(company_id, channel_id)
            .await
    }

    pub fn bindings(&self) -> &Arc<dyn ChannelBindingPersistence> {
        &self.bindings
    }

    pub fn installations(&self) -> &Arc<dyn InstallationPersistence> {
        &self.installations
    }
}

#[cfg(test)]
mod tests {
    use crate::application::ports::integration::InboundEndpoint;
    use crate::entities::transport::{
        BindingAccessPolicy, BindingAccessSnapshot, BindingAuditAction, BindingAuditEvent,
        BindingChangeReason, BindingDeliveryPolicy, BindingStatus, ChannelBindingId,
        EndpointNamespace, ExternalEndpointKey,
    };
    use std::sync::Mutex;

    use chrono::Utc;

    use super::*;

    /// Records what reached persistence, and answers installation lookups from a fixed status, so
    /// the rule under test is the use case's and not the database's.
    struct StubIntegrations {
        installation: Option<IntegrationInstallation>,
        created: Mutex<Vec<BindingWrite>>,
    }

    impl StubIntegrations {
        fn with_installation(status: InstallationStatus) -> Self {
            Self {
                installation: Some(IntegrationInstallation {
                    id: InstallationId::random(),
                    company_id: Uuid::new_v4(),
                    transport: TransportKind::Slack,
                    external_tenant_key: ExternalTenantKey::parse("T1").unwrap(),
                    display_name: "Acme".into(),
                    status,
                    granted_scopes: Vec::new(),
                    installed_by: CreationProvenance::system(),
                    installed_at: Utc::now(),
                    updated_by: CreationProvenance::system(),
                    updated_at: Utc::now(),
                    revoked_by: None,
                    revoked_at: None,
                }),
                created: Mutex::new(Vec::new()),
            }
        }

        fn without_installation() -> Self {
            Self {
                installation: None,
                created: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl InstallationPersistence for StubIntegrations {
        async fn install(&self, _: InstallationWrite) -> AppResult<IntegrationInstallation> {
            unreachable!("linking never installs")
        }

        async fn get_installation(
            &self,
            _: Uuid,
            _: InstallationId,
        ) -> AppResult<Option<IntegrationInstallation>> {
            Ok(self.installation.clone())
        }

        async fn find_installation_by_tenant(
            &self,
            _: TransportKind,
            _: &ExternalTenantKey,
        ) -> AppResult<Option<IntegrationInstallation>> {
            Ok(self.installation.clone())
        }

        async fn list_installations(&self, _: Uuid) -> AppResult<Vec<IntegrationInstallation>> {
            Ok(self.installation.clone().into_iter().collect())
        }

        async fn set_installation_status(
            &self,
            _: InstallationStatusChange,
        ) -> AppResult<IntegrationInstallation> {
            unreachable!("linking never changes an installation")
        }
    }

    #[async_trait]
    impl ChannelBindingPersistence for StubIntegrations {
        async fn create_binding(&self, write: BindingWrite) -> AppResult<ChannelBinding> {
            let binding = ChannelBinding {
                id: ChannelBindingId::random(),
                company_id: write.company_id,
                channel_id: write.channel_id,
                installation_id: write.installation_id,
                transport: write.transport,
                namespace: write.namespace.clone(),
                external_endpoint_key: write.external_endpoint_key.clone(),
                display_label: write.display_label.clone(),
                access_policy: write.access_policy,
                delivery_policy: write.delivery_policy,
                status: BindingStatus::Active,
                disabled_reason: None,
                created_by: write.created_by.clone(),
                access_snapshot: write.access_snapshot.clone(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            self.created.lock().unwrap().push(write);
            Ok(binding)
        }

        async fn active_bindings_for_channel(
            &self,
            _: Uuid,
            _: Uuid,
        ) -> AppResult<Vec<ChannelBinding>> {
            Ok(Vec::new())
        }

        async fn find_active_binding_by_endpoint(
            &self,
            _: &InboundEndpoint,
        ) -> AppResult<Option<ChannelBinding>> {
            Ok(None)
        }

        async fn list_bindings_for_company(&self, _: Uuid) -> AppResult<Vec<ChannelBinding>> {
            Ok(Vec::new())
        }

        async fn get_binding(
            &self,
            _: Uuid,
            _: ChannelBindingId,
        ) -> AppResult<Option<ChannelBinding>> {
            Ok(None)
        }

        async fn set_binding_status(&self, _: BindingStatusChange) -> AppResult<ChannelBinding> {
            unreachable!("this stub is only used for linking")
        }

        async fn list_binding_audit_events(
            &self,
            _: Uuid,
            _: ChannelBindingId,
            _: i64,
        ) -> AppResult<Vec<BindingAuditEvent>> {
            Ok(Vec::new())
        }
    }

    fn integrations(stub: StubIntegrations) -> (IntegrationUseCases, Arc<StubIntegrations>) {
        let stub = Arc::new(stub);
        (IntegrationUseCases::new(stub.clone(), stub.clone()), stub)
    }

    fn slack_write(installation_id: InstallationId) -> BindingWrite {
        BindingWrite {
            installation_id: Some(installation_id),
            transport: TransportKind::Slack,
            namespace: EndpointNamespace::parse("T1").unwrap(),
            external_endpoint_key: ExternalEndpointKey::parse("C1").unwrap(),
            display_label: "#support".into(),
            access_policy: BindingAccessPolicy::ConversationMembersReadAndParticipate,
            delivery_policy: BindingDeliveryPolicy::ReplyOnly,
            access_snapshot: BindingAccessSnapshot::provider_conversation(true, false, 3),
            ..write(TransportKind::Slack, Some(installation_id))
        }
    }

    #[tokio::test]
    async fn linking_through_a_lapsed_installation_is_refused_before_the_write() {
        for status in [
            InstallationStatus::ReauthorizationRequired,
            InstallationStatus::Revoked,
            InstallationStatus::Disabled,
        ] {
            let (use_cases, stub) = integrations(StubIntegrations::with_installation(status));
            let installation_id = stub.installation.as_ref().unwrap().id;

            let refused = use_cases.link_binding(slack_write(installation_id)).await;

            assert!(
                matches!(refused, Err(AppError::BadRequest(_))),
                "linking through a '{status}' installation must be refused"
            );
            assert!(
                stub.created.lock().unwrap().is_empty(),
                "nothing reaches persistence when the installation cannot be used"
            );
        }
    }

    #[tokio::test]
    async fn linking_names_an_installation_that_exists_and_is_active() {
        let (use_cases, stub) = integrations(StubIntegrations::with_installation(
            InstallationStatus::Active,
        ));
        let installation_id = stub.installation.as_ref().unwrap().id;

        let linked = use_cases
            .link_binding(slack_write(installation_id))
            .await
            .unwrap();

        assert_eq!(linked.installation_id, Some(installation_id));
        assert_eq!(stub.created.lock().unwrap().len(), 1);

        let (missing_use_cases, missing_stub) =
            integrations(StubIntegrations::without_installation());
        let unknown = missing_use_cases
            .link_binding(slack_write(InstallationId::random()))
            .await;
        assert!(matches!(unknown, Err(AppError::NotFound(_))));
        assert!(missing_stub.created.lock().unwrap().is_empty());
    }

    /// A deployment binding needs no installation, so the use case must not go looking for one.
    #[tokio::test]
    async fn an_email_binding_links_without_consulting_any_installation() {
        let (use_cases, stub) = integrations(StubIntegrations::without_installation());

        let linked = use_cases
            .link_binding(write(TransportKind::Email, None))
            .await
            .unwrap();

        assert_eq!(linked.installation_id, None);
        assert_eq!(stub.created.lock().unwrap().len(), 1);
    }

    fn write(transport: TransportKind, installation_id: Option<InstallationId>) -> BindingWrite {
        BindingWrite {
            company_id: Uuid::new_v4(),
            channel_id: Uuid::new_v4(),
            installation_id,
            transport,
            namespace: EndpointNamespace::parse("email").unwrap(),
            external_endpoint_key: ExternalEndpointKey::parse("support@acme").unwrap(),
            display_label: "Support".into(),
            access_policy: BindingAccessPolicy::ChannelAcl,
            delivery_policy: BindingDeliveryPolicy::ReplyAndInitiate,
            access_snapshot: BindingAccessSnapshot::deployment_endpoint(),
            created_by: CreationProvenance::system(),
        }
    }

    #[test]
    fn a_binding_names_an_installation_exactly_when_its_transport_needs_one() {
        assert!(write(TransportKind::Email, None).validate().is_ok());
        assert!(
            write(TransportKind::Slack, Some(InstallationId::random()))
                .validate()
                .is_ok()
        );
        assert!(write(TransportKind::Slack, None).validate().is_err());
        assert!(
            write(TransportKind::Email, Some(InstallationId::random()))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn only_a_binding_that_stops_carrying_traffic_takes_a_reason() {
        let change = |status, reason| BindingStatusChange {
            company_id: Uuid::new_v4(),
            binding_id: ChannelBindingId::random(),
            status,
            reason,
            actor: CreationProvenance::system(),
        };

        assert!(change(BindingStatus::Active, None).validate().is_ok());
        assert!(
            change(
                BindingStatus::Disabled,
                Some(BindingChangeReason::ManagerRequest)
            )
            .validate()
            .is_ok()
        );
        assert!(change(BindingStatus::Disabled, None).validate().is_err());
        assert!(
            change(
                BindingStatus::Paused,
                Some(BindingChangeReason::ManagerRequest)
            )
            .validate()
            .is_err()
        );
    }

    #[test]
    fn every_status_transition_has_exactly_one_audit_verb() {
        let action = |status| {
            BindingStatusChange {
                company_id: Uuid::new_v4(),
                binding_id: ChannelBindingId::random(),
                status,
                reason: None,
                actor: CreationProvenance::system(),
            }
            .audit_action()
        };

        assert_eq!(action(BindingStatus::Active), BindingAuditAction::Enabled);
        assert_eq!(action(BindingStatus::Paused), BindingAuditAction::Paused);
        assert_eq!(
            action(BindingStatus::Disabled),
            BindingAuditAction::Disabled
        );
        assert_eq!(
            action(BindingStatus::Orphaned),
            BindingAuditAction::DriftDetected
        );
    }
}
