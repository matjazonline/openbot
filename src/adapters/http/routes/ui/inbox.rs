//! Parse and authorize one exact typed mailbox target for every reader and writer.
use super::*;
use crate::entities::{address::AddressTarget, transport::PrincipalId};
use crate::use_cases::thread::InboxReadScope;

#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct InboxSelection {
    pub channel_id: Option<Uuid>,
    pub principal_id: Option<PrincipalId>,
}
impl InboxSelection {
    pub fn target(self) -> AppResult<Option<AddressTarget>> {
        match (self.channel_id, self.principal_id) {
            (None, None) => Ok(None),
            (Some(id), None) => Ok(Some(AddressTarget::Desk(id))),
            (None, Some(id)) => Ok(Some(AddressTarget::Principal(id))),
            _ => Err(AppError::BadRequest("Select exactly one inbox".into())),
        }
    }
    pub fn scope(self, company_id: Uuid) -> AppResult<InboxReadScope> {
        Ok(InboxReadScope {
            company_id,
            target: self
                .target()?
                .ok_or_else(|| AppError::BadRequest("Select an inbox".into()))?,
        })
    }
}

pub(crate) async fn load_viewable_inbox(
    companies: &CompanyUseCases,
    threads: &ThreadUseCases,
    viewer: &Viewer,
    company_id: Uuid,
    selection: InboxSelection,
) -> AppResult<(Company, ReadableInbox)> {
    let inbox = threads
        .readable_inbox(viewer, selection.scope(company_id)?)
        .await?;
    let company = companies
        .readable_company(viewer.user_id, company_id)
        .await?
        .company;
    Ok((company, inbox))
}

pub(crate) fn send_address(
    inbox: &ReadableInbox,
    company: &Company,
    domain: &str,
) -> AppResult<EmailAddress> {
    let slug = inbox
        .slug()
        .ok_or_else(|| AppError::NotFound("Inbox has no active address".into()))?;
    Ok(crate::entities::address::address_for(
        slug,
        &company.slug,
        domain,
    ))
}
