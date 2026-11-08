from pathlib import Path
p=Path('src/adapters/http/pages/mod.rs');s=p.read_text().replace('mod team_settings;','mod team_settings;\nmod team_levels;').replace('pub use team_settings::*;','pub use team_settings::*;\npub use team_levels::*;');p.write_text(s)
p=Path('src/adapters/http/pages/team_settings.rs');s=p.read_text();s='use crate::entities::access_level::{AccessLevel, LevelActor, LevelTarget};\nuse super::team_levels::{LevelControl, level_control, level_options};\n'+s if not s.startswith('//!') else s
# Keep module documentation first.
if 'use super::team_levels::' not in s:
 marker='#[derive(Debug, Clone, Copy, PartialEq, Eq)]';s=s.replace(marker,'use crate::entities::access_level::{AccessLevel, LevelActor, LevelTarget};\nuse super::team_levels::{LevelControl, level_control, level_options};\n\n'+marker,1)
s=s.replace('pub enum TeamRole {\n    Owner,','pub enum TeamRole {\n    Owner,\n    Admin,',1)
pos=s.index('/// Which sidebar entry')
s=s[:pos]+'''impl TeamRole {
    pub fn manages_levels(self) -> bool { matches!(self, Self::Owner | Self::Admin) }
    pub fn level_actor(self, access_level: AccessLevel) -> LevelActor {
        LevelActor { membership: match self {
            Self::Owner => crate::entities::company_member::CompanyMembership::Owner,
            Self::Admin => crate::entities::company_member::CompanyMembership::Admin,
            Self::Member => crate::entities::company_member::CompanyMembership::Member,
        }, access_level }
    }
}

'''+s[pos:]
for name in ('MemberPane','InvitePane'):
 start=s.index('pub struct '+name);end=s.index('\n}',start);part=s[start:end];part=part.replace('pub role: TeamRole,','pub role: TeamRole,\n    pub viewer_access_level: AccessLevel,');s=s[:start]+part+s[end:]
s=s.replace('let invites = if list.role.manages() {','let invites = if list.role.manages_levels() {',1)
# Sidebar labels disclose each stored level without changing role powers.
s=s.replace('<span class="badge badge-ghost badge-sm shrink-0">{role}</span>','<span class="badge badge-ghost badge-sm shrink-0">{role} · {level}</span>',1)
s=s.replace('role = escape_html_text(role),','role = escape_html_text(role),\n        level = member.access_level.label(),',1)
s=s.replace('<span class="badge badge-ghost badge-sm ml-auto shrink-0">{role}</span>','<span class="badge badge-ghost badge-sm ml-auto shrink-0">{role} · {level}</span>',1)
s=s.replace('role = invite.role.label(),','role = invite.role.label(),\n        level = invite.access_level.label(),',1)
s=s.replace('                {access_role_form}\n','                {access_role_form}\n                {access_level_form}\n',1)
s=s.replace('access_role_form = member_access_role_form(pane),','''access_role_form = member_access_role_form(pane),
        access_level_form = level_control(LevelControl {
            actor: pane.role.level_actor(pane.viewer_access_level),
            target: if is_owner { LevelTarget::Owner } else { LevelTarget::Assignable(member.access_level) },
            current: member.access_level,
            endpoint: &team_endpoint(pane.company.id, &format!("/members/{}/level", member.user_id)),
            swap_target: "#team-pane",
        }),''',1)
# Extract existing >80-line invitation body before adding the level rendering.
start=s.index('    // An invite that has already been answered');end=s.index('\n    format!(\n',s.index('\n    };',start))
body=s[start:end];body=body.replace('    let body = if','    if',1);body=body.rsplit('    };',1)[0]+'    }\n'
helper='''fn invite_details(pane: &InvitePane<'_>) -> String {
    let invite = pane.invite;
    let status = InviteStatus::parse(&invite.status);
    let email = pane.email_draft.unwrap_or(&invite.email);
    let role = pane.role_draft.unwrap_or(invite.role);
    let invite_endpoint = team_endpoint(pane.company.id, &format!("/invites/{}", invite.id));
    let close_endpoint = team_endpoint(pane.company.id, "/close");
    let cleared_url = team_url(pane.company.id, TeamSelection::None);
'''+body+'}\n\n'
s=s[:start]+'    let body = invite_details(pane);\n'+s[end:]
start=s.index('pub fn invite_pane(');end=s.index('\npub fn invite_create_pane',start);part=s[start:end]
for line in ['    let email = pane.email_draft.unwrap_or(&invite.email);\n','    let role = pane.role_draft.unwrap_or(invite.role);\n','    let close_endpoint = team_endpoint(company_id, "/close");\n','    let cleared_url = team_url(company_id, TeamSelection::None);\n']:
 part=part.replace(line,'')
part=part.replace('                {body}\n','                {body}\n                {level}\n',1)
part=part.replace('        error_html = form_error_banner(pane.error),','''        error_html = form_error_banner(pane.error),
        level = if status == InviteStatus::Pending {
            level_control(LevelControl {
                actor: pane.role.level_actor(pane.viewer_access_level),
                target: LevelTarget::Assignable(invite.access_level), current: invite.access_level,
                endpoint: &format!("{invite_endpoint}/level"), swap_target: "#team-pane",
            })
        } else { format!("<p>{}</p>", invite.access_level.label()) },''',1)
s=s[:start]+helper+part+s[end:]
# Pending admins get read-only details, not misleading answered text.
s=s.replace('''    } else {
        let delete_button = if pane.role.manages()''','''    } else if status == InviteStatus::Pending {
        format!("<p>Pending invitation · {}</p>", role.label())
    } else {
        let delete_button = if pane.role.manages()''',1)
# Owner-only create form includes all legal levels; omitted HTTP input still means public.
start=s.index('pub fn invite_create_pane');part=s[start:];part=part.replace('                    <div class="flex items-center gap-3">','''                    <label class="form-control"><span>Access level</span><select name="access_level" class="select">{level_options}</select></label>
                    <p class="text-xs opacity-60">{level_help}</p>
                    <div class="flex items-center gap-3">''',1);part=part.replace('role_options = access_role_options(pane.role_draft),','''role_options = access_role_options(pane.role_draft),
        level_options = level_options(TeamRole::Owner.level_actor(AccessLevel::MAX), LevelTarget::Assignable(AccessLevel::PUBLIC), AccessLevel::PUBLIC),
        level_help = super::team_levels::LEVEL_HELP,''',1);s=s[:start]+part;p.write_text(s)
# Team view keeps the authoritative level from the exact membership read.
p=Path('src/adapters/http/routes/ui_team.rs');s=p.read_text();s=s.replace('membership: access.membership,','membership: access.membership,\n            access_level: access.access_level,',1);s=s.replace('membership: CompanyMembership,','membership: CompanyMembership,\n    access_level: crate::entities::access_level::AccessLevel,',1)
s=s.replace('''        } else {
            TeamRole::Member
        }
    }

    /// The owner manages''','''        } else if self.membership == CompanyMembership::Admin {
            TeamRole::Admin
        } else { TeamRole::Member }
    }

    /// The owner manages''',1)
s=s.replace('TeamRole::Member => Err(AppError::Forbidden(', 'TeamRole::Admin | TeamRole::Member => Err(AppError::Forbidden(',1)
s=s.replace('TeamRole::Owner => {\n                self.invite_use_cases','TeamRole::Owner | TeamRole::Admin => {\n                self.invite_use_cases',1)
start=s.index('    async fn member(&self,');end=s.index('\n    async fn invite(',start);s=s[:start]+'''    async fn member(&self, user_id: Uuid) -> AppResult<CompanyMember> {
        self.invite_use_cases.get_company_team_member(self.user_id, self.company.id, user_id).await?
            .ok_or_else(|| AppError::NotFound("Team member not found".into()))
    }
'''+s[end:]
# Insert level facts only in pane constructors, not sidebar which has no viewer-level control.
for name in ('MemberPane','InvitePane'):
 marker='pages::'+name+' {';start=s.index(marker);end=s.index('\n        })',start);part=s[start:end].replace('role: self.role(),','role: self.role(),\n            viewer_access_level: self.access_level,');s=s[:start]+part+s[end:]
s=s.replace('        // Removal. A POST', '''        .route("/ui/companies/{company_id}/team/members/{user_id}/level", put(set_member_level))
        .route("/ui/companies/{company_id}/team/invites/{invite_id}/level", put(set_invite_level))
        // Removal. A POST''',1)
# Keep large router file under its existing module boundary.
s+='''
#[path = "ui_team_levels.rs"]
mod levels;
use levels::{set_member_level, set_invite_level};
''';p.write_text(s)
# Fix D3 compiler's omitted form fixtures and explicit public defaults.
for filename in ('src/adapters/http/routes/company_invite.rs','src/adapters/http/routes/ui_team.rs'):
 p=Path(filename);s=p.read_text().replace('access_level.unwrap_or_default()', 'access_level.unwrap_or(crate::entities::access_level::AccessLevel::PUBLIC)');p.write_text(s)
for filename in ('src/adapters/http/routes/company_invite_authorization_tests.rs','src/adapters/http/routes/ui_team_authorization_tests.rs'):
 p=Path(filename);s=p.read_text().replace('Form(InviteForm {','Form(InviteForm {\n                access_level: None,');p.write_text(s)
p=Path('src/adapters/persistence/company_invite_tests.rs');s=p.read_text().replace('get_invite_by_id(invite.id)','get_invite_by_id(invite.company_id, invite.id)').replace('get_invite_by_id(invite_id)','get_invite_by_id(fixture.invite.company_id, invite_id)');p.write_text(s)
p=Path('src/adapters/persistence/company_owner_tests.rs');s=p.read_text().replace('get_invite_by_id(invite.id)','get_invite_by_id(fixture.company_id, invite.id)');p.write_text(s)
# Pane tests declare the actual role's matching fixture level.
p=Path('src/adapters/http/pages/tests.rs');s=p.read_text()
import re
for name in ('MemberPane','InvitePane'):
 for match in reversed(list(re.finditer(r'\b'+name+r'\s*\{',s))):
  end=s.index('\n    }',match.end());part=s[match.start():end]
  if 'viewer_access_level:' in part:continue
  part=re.sub(r'role: TeamRole::(Owner|Member),',lambda m:m[0]+'\n        viewer_access_level: crate::entities::access_level::AccessLevel::'+('MAX' if m[1]=='Owner' else 'PUBLIC')+',',part)
  s=s[:match.start()]+part+s[end:]
p.write_text(s)
