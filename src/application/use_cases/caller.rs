//! The checks a use case makes on [`Caller`] itself, as opposed to on a company membership.

use crate::{
    app_error::{AppError, AppResult},
    entities::user::Caller,
};

/// `Ok` for a deployment operator. Everyone else is told the surface does not exist, exactly as
/// for an unknown id: an operator workspace is not something a tenant user may learn is there.
pub fn require_operator(caller: &Caller) -> AppResult<()> {
    if caller.is_operator() {
        Ok(())
    } else {
        Err(AppError::NotFound("Operator workspace not found.".into()))
    }
}
