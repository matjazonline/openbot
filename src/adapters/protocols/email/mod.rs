pub mod egress;
pub mod ingress;
mod selector;
mod types;

pub use egress::EmailEgressAdapter;
pub use ingress::EmailIngressAdapter;
pub use selector::{
    EmailChannelSelection, EmailChannelSelectorParser, EmailDeliveryHints, EmailDeliveryMode,
    EmailRecipientDestination,
};
pub use types::{EmailIdentity, EmailIdentityError, EmailMessageKey};
