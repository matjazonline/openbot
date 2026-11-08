from pathlib import Path
import re
ROOT=Path('.')
ns='crate::use_cases::company_invite::'
public='crate::entities::access_level::AccessLevel::PUBLIC'

def calls(text, method):
    for m in re.finditer(r'\.'+method+r'\s*\(', text):
        start=m.end(); depth=0; quoted=False; escape=False; args=[]; last=start
        for i in range(start,len(text)):
            c=text[i]
            if quoted:
                if escape: escape=False
                elif c=='\\': escape=True
                elif c=='"': quoted=False
                continue
            if c=='"': quoted=True
            elif c in '([{': depth+=1
            elif c in ')]}':
                if c==')' and depth==0:
                    if text[last:i].strip(): args.append(text[last:i].strip())
                    yield m.start(),i+1,args
                    break
                depth-=1
            elif c==',' and depth==0:
                args.append(text[last:i].strip()); last=i+1

def replace_calls(p, method, change):
    text=p.read_text(); edits=[]
    for start,end,args in calls(text,method):
        replacement=change(text,start,args)
        if replacement is not None: edits.append((start,end,'.'+method+'('+replacement+')'))
    for start,end,replacement in reversed(edits): text=text[:start]+replacement+text[end:]
    if edits: p.write_text(text)

for p in ROOT.glob('src/**/*.rs'):
    def create(text,start,a):
        if len(a)!=4:return
        actor,company,email,role=a
        level=next((x+'.access_level.unwrap_or_default()' for x in ('form','payload') if email=='&'+x+'.email'),public)
        return ns+'CreateCompanyInvite { actor_id: '+actor+', company_id: '+company+', email: ('+email+').to_string().into(), role: '+role+', access_level: '+level+' }'
    def update(text,start,a):
        if len(a)!=5:return
        actor,company,invite,email,role=a
        level=next((x+'.access_level' for x in ('form','payload') if email=='&'+x+'.email'),'None')
        return ns+'UpdateCompanyInvite { actor_id: '+actor+', company_id: '+company+', invite_id: '+invite+', email: ('+email+').to_string().into(), role: '+role+', access_level: '+level+' }'
    replace_calls(p,'create_company_invite',create)
    replace_calls(p,'update_company_invite',update)
    if ('tests' in p.name or p.name=='company.rs') and 'persistence' in str(p) or p.name in ('authorization_test_support.rs','ui_run_counts_tests.rs','company_invite_authorization_tests.rs'):
        def native(text,start,a):
            if len(a)!=3:return
            receiver=re.search(r'([\w]+(?:\s*\.\s*[\w]+)*)\s*$',text[:start]).group(1)
            return 'crate::adapters::persistence::test_support::invite_command(&'+receiver+', '+', '.join(a)+').await'
        def native_update(text,start,a):
            if len(a)!=4:return
            receiver=re.search(r'([\w]+(?:\s*\.\s*[\w]+)*)\s*$',text[:start]).group(1)
            return 'crate::adapters::persistence::test_support::invite_update_command(&'+receiver+', '+', '.join(a)+').await'
        replace_calls(p,'create_invite',native)
        replace_calls(p,'update_invite',native_update)

p=Path('src/adapters/persistence/company_invite.rs');s=p.read_text().replace('use_cases::company_invite::CompanyInvitePersistence,','use_cases::company_invite::{CompanyInvitePersistence, CreateCompanyInvite, UpdateCompanyInvite, MemberLevelChange, InviteLevelChange, invite_address},');p.write_text(s)
p=Path('src/adapters/persistence/company_level_authority.rs');s=p.read_text().replace('use crate::entities::company_member::CompanyMembership;\n','');p.write_text(s)
p=Path('src/application/use_cases/company_invite.rs');s=p.read_text();old='''let access = owned_company(
            self.company_persistence.as_ref(),
            command.actor_id,
            command.company_id,
        )
        .await?;''';new='''let access = self.level_manager(command.actor_id, command.company_id).await?;
        if !access.membership.is_owner() {
            return Err(AppError::Forbidden("Only the company owner can do this.".into()));
        }''';assert s.count(old)==2;s=s.replace(old,new);p.write_text(s)
for file in ('src/adapters/http/routes/company_invite.rs','src/adapters/http/routes/ui_team.rs'):
    p=Path(file);s=p.read_text();marker='pub struct InviteForm {\n    pub email: String,';assert marker in s;s=s.replace(marker,marker+'\n    pub access_level: Option<crate::entities::access_level::AccessLevel>,',1);p.write_text(s)
p=Path('src/application/use_cases/company_invite_tests.rs');s=p.read_text();start=s.index('    async fn create_invite(');end=s.index('    async fn list_invites_by_company',start)
s=s[:start]+'''    async fn create_invite(&self, command: CreateCompanyInvite) -> AppResult<CompanyInvite> {
        let invite = CompanyInvite {
            id: Uuid::new_v4(), company_id: command.company_id,
            company_name: Some("Acme".into()), email: command.email.to_string(),
            role: command.role, access_level: command.access_level,
            status: "pending".into(), created_at: Utc::now(),
        };
        self.invites.lock().unwrap().push(invite.clone());
        Ok(invite)
    }
    async fn get_invite_by_id(&self, company_id: Uuid, id: Uuid) -> AppResult<Option<CompanyInvite>> {
        Ok(self.invites.lock().unwrap().iter().find(|i| i.id == id && i.company_id == company_id).cloned())
    }

'''+s[end:]
start=s.index('    async fn update_invite(');end=s.index('    async fn delete_invite',start)
s=s[:start]+'''    async fn update_invite(&self, command: UpdateCompanyInvite) -> AppResult<Option<CompanyInvite>> {
        let mut list = self.invites.lock().unwrap();
        let Some(invite) = list.iter_mut().find(|i| i.id == command.invite_id && i.company_id == command.company_id && i.status == "pending") else { return Ok(None) };
        invite.email = command.email.to_string();
        if let Some(role) = command.role { invite.role = role; }
        if let Some(level) = command.access_level { invite.access_level = level; }
        Ok(Some(invite.clone()))
    }
    async fn get_member_by_user_id(&self, company_id: Uuid, user_id: Uuid) -> AppResult<Option<CompanyMember>> {
        Ok(self.members.lock().unwrap().iter().find(|m| m.company_id == company_id && m.user_id == user_id).cloned())
    }
    async fn set_member_access_level(&self, _change: MemberLevelChange) -> AppResult<Option<CompanyMember>> {
        Err(AppError::Internal("Level writes require the native authority fixture".into()))
    }
    async fn set_invite_access_level(&self, _change: InviteLevelChange) -> AppResult<Option<CompanyInvite>> {
        Err(AppError::Internal("Level writes require the native authority fixture".into()))
    }

'''+s[end:]
s=s.replace('access_level: crate::entities::access_level::AccessLevel::PUBLIC,\n                id:', 'access_level: invite.access_level,\n                id:',1);p.write_text(s)
p=Path('src/adapters/persistence/test_support.rs');s=p.read_text();s+='''
/// Native fixture commands carry the actual owner, rather than bypassing writer authority.
pub async fn invite_command(
    persistence: &super::PostgresPersistence, company_id: Uuid, email: &str,
    role: crate::entities::company_member::CompanyAccessRole,
) -> crate::use_cases::company_invite::CreateCompanyInvite {
    let actor_id = sqlx::query_scalar("SELECT user_id FROM companies WHERE id=$1")
        .bind(company_id).fetch_one(&persistence.pool).await.unwrap();
    crate::use_cases::company_invite::CreateCompanyInvite {
        actor_id, company_id, email: email.into(), role,
        access_level: crate::entities::access_level::AccessLevel::PUBLIC,
    }
}
pub async fn invite_update_command(
    persistence: &super::PostgresPersistence, company_id: Uuid, invite_id: Uuid, email: &str,
    role: crate::entities::company_member::CompanyAccessRole,
) -> crate::use_cases::company_invite::UpdateCompanyInvite {
    let actor_id = sqlx::query_scalar("SELECT user_id FROM companies WHERE id=$1")
        .bind(company_id).fetch_one(&persistence.pool).await.unwrap();
    crate::use_cases::company_invite::UpdateCompanyInvite {
        actor_id, company_id, invite_id, email: email.into(), role: Some(role), access_level: None,
    }
}
''';p.write_text(s)
