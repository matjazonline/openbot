from pathlib import Path
p=Path('src/application/use_cases/company_level_use_cases.rs');s=p.read_text().replace('pub(super) async fn level_manager','pub(crate) async fn level_manager');p.write_text(s)
p=Path('src/adapters/http/routes/company_invite.rs');s=p.read_text();needle='        // Web / HTMX User Invites';s=s.replace(needle,'''        .route("/companies/{company_id}/team/{user_id}/level", put(levels::set_member_level))
        .route("/companies/{company_id}/invites/{invite_id}/level", put(levels::set_invite_level))
        .route("/api/companies/{company_id}/team/{user_id}/level", put(levels::set_member_level_json))
        .route("/api/companies/{company_id}/invites/{invite_id}/level", put(levels::set_invite_level_json))
'''+needle,1);s+='''
#[path = "company_invite_levels.rs"]
mod levels;
pub(super) use levels::LevelForm;
'''
s=s.replace('let company = company_use_cases.owned_company(user.id, company_id).await?;','let access = company_use_cases.readable_company(user.id, company_id).await?;\n    let company = &access.company;\n    let actor = pages::team_level_actor(&access);',1)
s=s.replace('&company, &invites, &members,','company, &invites, &members, actor,',1)
# Each browser response has a current joint company access; no hard-coded owner rendering.
for name in ('create_company_invite','edit_invite_form','cancel_invite_edit','update_company_invite','update_team_member_role'):
 start=s.index('async fn '+name+'(');brace=s.index(') ->',start);body=s.index('{',brace)+1
 s=s[:body]+'''\n    let access = invite_use_cases.level_manager(user.id, company_id).await?;
    let actor = pages::team_level_actor(&access);'''+s[body:]
# Narrow call transformations on this file only.
s=s.replace('pages::company_invite_list_fragment(company_id, &invites)','pages::company_invite_list_fragment(company_id, &invites, actor)')
s=s.replace('company_id, &invite,\n','company_id, &invite, actor,\n')
s=s.replace('pages::company_invite_row_fragment(company_id, &invite)','pages::company_invite_row_fragment(company_id, &invite, actor)')
s=s.replace('pages::company_team_row_fragment(company_id, &member)','pages::company_team_row_fragment(company_id, &member, actor)')
p.write_text(s)
# Legacy server pages take the same LevelActor as the shared pure rule.
p=Path('src/adapters/http/pages/companies.rs');s=p.read_text();s='use crate::entities::access_level::{AccessLevel, LevelActor, LevelTarget};\nuse super::team_levels::{LevelControl, level_control, level_options, LEVEL_HELP};\n'+s
s=s.replace('    members: &[CompanyMember],\n) -> String {','    members: &[CompanyMember],\n    actor: LevelActor,\n) -> String {',1)
for name,ty in (('company_invite_list_fragment','invites: &[CompanyInvite]'),('company_invite_row_fragment','invite: &CompanyInvite'),('company_invite_edit_fragment','invite: &CompanyInvite'),('company_team_list_fragment','members: &[CompanyMember]'),('company_team_row_fragment','member: &CompanyMember')):
 old='pub fn '+name+'(company_id: Uuid, '+ty+') -> String {';assert old in s,old;s=s.replace(old,old.replace(') ->',', actor: LevelActor) ->'),1)
s=s.replace('company_invite_list_fragment(company.id, invites)','company_invite_list_fragment(company.id, invites, actor)').replace('company_team_list_fragment(company.id, members)','company_team_list_fragment(company.id, members, actor)').replace('company_invite_row_fragment(company_id, inv)','company_invite_row_fragment(company_id, inv, actor)').replace('company_team_row_fragment(company_id, m)','company_team_row_fragment(company_id, m, actor)')
start=s.index('        <!-- Create Invite Card -->');end=s.index('        <!-- Invites List Section -->',start);card=s[start:end];s=s[:start]+'        {create_invite}\n\n'+s[end:]
s=s.replace('role_options = company_access_role_options(CompanyAccessRole::Member),','create_invite = legacy_invite_create(company.id, actor),',1)
card=card.replace('                <button type="submit"','                <select aria-label="Access level" name="access_level" class="select">{levels}</select>\n                <button type="submit"',1).replace('        </div>\n\n','            <p class="text-xs opacity-60">{LEVEL_HELP}</p>\n        </div>\n\n',1)
s+='''
fn legacy_invite_create(company_id: Uuid, actor: LevelActor) -> String {
    if !actor.membership.is_owner() { return String::new(); }
    format!(r##"'''+card+'''"##,
        role_options=company_access_role_options(CompanyAccessRole::Member),
        levels=level_options(actor, LevelTarget::Assignable(AccessLevel::PUBLIC), AccessLevel::PUBLIC))
}
'''
# Pull row controls into values so admins never receive owner-only role/email/delete elements.
start=s.index('pub fn company_invite_row_fragment');end=s.index('\npub fn company_invite_edit_fragment',start);part=s[start:end];a=part.index('            <div class="flex items-center gap-2">');b=part.index('\n            </div>',a)+len('\n            </div>');actions=part[a:b];part=part[:a]+'            {level}\n            {actions}'+part[b:];part=part.replace('created_at_str = created_at_str,','''created_at_str = created_at_str,
        level = if invite.status == "pending" { level_control(LevelControl {
            actor, target: LevelTarget::Assignable(invite.access_level), current: invite.access_level,
            endpoint: &format!("/companies/{company_id}/invites/{}/level", invite.id),
            swap_target: &format!("#invite-{}", invite.id),
        }) } else { format!("<p>{}</p>", invite.access_level.label()) },
        actions = if actor.membership.is_owner() { format!(r##"'''+actions+'''"##,
            invite_id=invite.id, email=escape_html_attr(&invite.email)) } else { String::new() },''',1);s=s[:start]+part+s[end:]
# Direct admin GET of the legacy edit URL is a level-only row.
start=s.index('pub fn company_invite_edit_fragment');brace=s.index('{',start)+1;s=s[:brace]+'''
    if !actor.membership.is_owner() {
        return company_invite_row_fragment(company_id, invite, actor);
    }
'''+s[brace:]
# Owner edit selects its current level, processed atomically alongside address/role.
start=s.index('pub fn company_invite_edit_fragment');end=s.index('\npub fn company_team_list_fragment',start);part=s[start:end].replace('            <div class="flex items-center gap-2">','            <select aria-label="Access level" name="access_level" class="select">{levels}</select>\n            <div class="flex items-center gap-2">',1).replace('role_options = company_access_role_options(invite.role),','role_options = company_access_role_options(invite.role),\n        levels = level_options(actor, LevelTarget::Assignable(invite.access_level), invite.access_level),',1);s=s[:start]+part+s[end:]
start=s.index('pub fn company_team_row_fragment');end=s.index('\nfn company_access_role_options',start);part=s[start:end]
a=part.index('                    <form hx-put=');b=part.index('\n                    </form>',a)+len('\n                    </form>');roleform=part[a:b];part=part[:a]+'                    {role_form}'+part[b:]
a=part.index('            <div class="flex items-center gap-2">');b=part.index('\n            </div>',a)+len('\n            </div>');remove=part[a:b];part=part[:a]+'            {level}\n            {remove}'+part[b:]
part=part.replace('role_options = company_access_role_options(member.role),','''role_form = if actor.membership.is_owner() { format!(r##"'''+roleform+'''"##,
            user_id=member.user_id, role_options=company_access_role_options(member.role)) } else { format!("<span>{}</span>", member.role.label()) },
        remove = if actor.membership.is_owner() { format!(r##"'''+remove+'''"##,
            user_id=member.user_id, username=escape_html_attr(username_display)) } else { String::new() },
        level = level_control(LevelControl { actor, target: LevelTarget::Assignable(member.access_level), current: member.access_level,
            endpoint: &format!("/companies/{company_id}/team/{}/level",member.user_id), swap_target: &format!("#member-{}",member.user_id) }),''',1);s=s[:start]+part+s[end:];p.write_text(s)
