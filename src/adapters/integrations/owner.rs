//! Trusted local host identity mapping; never takes an owner from browser input.
use crate::{application::integrations::VerifiedOwner, domain::integrations::*};
use uuid::Uuid;
pub(crate) fn client_namespace() -> OwnerNamespace {
    OwnerNamespace::parse("openbot").expect("code-configured client namespace")
}
pub(crate) fn owner_for_user(user: Uuid) -> AccountOwner {
    AccountOwner {
        namespace: client_namespace(),
        subject: OwnerSubject::parse(user.to_string()).expect("UUID owner subject"),
    }
}
pub(crate) fn verified_owner(
    user: &crate::adapters::http::auth::AuthenticatedUser,
) -> VerifiedOwner {
    VerifiedOwner::from_verified_client(owner_for_user(user.id))
}
pub(crate) fn company_binding(company: Uuid) -> HostBinding {
    HostBinding::parse(format!("company/{company}")).expect("UUID host binding")
}
