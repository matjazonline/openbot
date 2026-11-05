//! Fresh classification facts for application read projections, never a channel-access grant.
use crate::entities::access_level::{AccessLevel, visible_to};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PayloadVisibility {
    pub viewer: AccessLevel,
    pub content: AccessLevel,
}
impl PayloadVisibility {
    pub const fn is_visible(self) -> bool {
        visible_to(self.viewer, self.content)
    }
}
