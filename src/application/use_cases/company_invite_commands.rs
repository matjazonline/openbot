//! Actor-bearing team commands. Roles and levels are resolved again by the persistence writer.
use crate::entities::{
    access_level::AccessLevel, company_member::CompanyAccessRole, value_objects::EmailAddress,
};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateCompanyInvite {
    pub actor_id: Uuid,
    pub company_id: Uuid,
    pub email: EmailAddress,
    pub role: CompanyAccessRole,
    pub access_level: AccessLevel,
}
#[derive(Debug, Clone)]
pub struct UpdateCompanyInvite {
    pub actor_id: Uuid,
    pub company_id: Uuid,
    pub invite_id: Uuid,
    pub email: EmailAddress,
    pub role: Option<CompanyAccessRole>,
    pub access_level: Option<AccessLevel>,
}
#[derive(Debug, Clone, Copy)]
pub struct MemberLevelChange {
    pub actor_id: Uuid,
    pub company_id: Uuid,
    pub member_user_id: Uuid,
    pub access_level: AccessLevel,
}
#[derive(Debug, Clone, Copy)]
pub struct InviteLevelChange {
    pub actor_id: Uuid,
    pub company_id: Uuid,
    pub invite_id: Uuid,
    pub access_level: AccessLevel,
}
