pub mod ingress;
mod selector;
mod types;

pub use ingress::EmailIngressAdapter;
pub use selector::{
    EmailChannelSelection, EmailChannelSelectorParser, EmailDeliveryHints, EmailDeliveryMode,
    EmailRecipientDestination,
};
pub use types::{
    EMAIL_IDENTITY_NAMESPACE, EmailEndpointKey, EmailIdentity, EmailIdentityError, EmailMessageKey,
};
