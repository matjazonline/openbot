from pathlib import Path
# Shared scoped fixtures for ordinary tests; only competing-lock/trigger tests own whole databases.
p=Path('src/adapters/persistence/company_access_level_test_support.rs');s=p.read_text().replace('    adapters::persistence::test_support::{OwnDatabase, own_database},\n','').replace('pub database: OwnDatabase,','pub pool: sqlx::PgPool,').replace('pub async fn new() -> Option<Self> {\n        let database = own_database().await?;\n        let persistence = Arc::new(PostgresPersistence::new(database.pool.clone()));','pub async fn new(pool: sqlx::PgPool) -> Self {\n        let persistence = Arc::new(PostgresPersistence::new(pool.clone()));').replace('            database,','            pool,').replace('        Some(fixture)','        fixture').replace('&self.database.pool','&self.pool');p.write_text(s)
p=Path('src/adapters/persistence/company_access_level_tests.rs');s=p.read_text().replace('use support::{Fixture, account};','use support::{Fixture, account};\nuse crate::adapters::persistence::test_support::test_pool;').replace('let Some(f) = Fixture::new().await else {','let Some(pool) = test_pool().await else {').replace('        return;\n    };','        return;\n    };\n    let f = Fixture::new(pool).await;');s=s.replace('&f.database.pool','&f.pool')
# Split the long native writer test at the actual membership / invitation boundary.
start=s.index('    let invite = f.invite(&stranger',s.index('async fn access_level_native_writers'));end=s.index('\n}\n',start);chunk=s[start:end];s=s[:start]+'    assert_invite_writer_authority(&f, &stranger).await;'+s[end:]
pos=s.index('\nfn email(');s=s[:pos]+'''
async fn assert_invite_writer_authority(f: &Fixture, stranger: &crate::entities::user::User) {
    let p = f.persistence.as_ref();
'''+chunk+'\n}\n'+s[pos:]
# Split current-reader projection checks from external/retirement behavior.
start=s.index('    struct Reader',s.index('async fn access_level_joint_readers'));end=s.index('    let external =',start);chunk=s[start:end];s=s[:start]+'    assert_current_readers(&f).await;\n'+s[end:];s+='''
async fn assert_current_readers(f: &Fixture) {
    let p=f.persistence.as_ref();
'''+chunk+'}\n';p.write_text(s)
p=Path('src/adapters/persistence/company_access_level_race_tests.rs');s=p.read_text().replace('wait_until_a_backend_is_blocked, wait_until_backends_are_blocked,','own_database, wait_until_a_backend_is_blocked, wait_until_backends_are_blocked,').replace('let Some(f) = Fixture::new().await else {','let Some(database) = own_database().await else {').replace('        return;\n    };','        return;\n    };\n    let f = Fixture::new(database.pool.clone()).await;').replace('&f.database.pool','&f.pool').replace('f.database.pool','f.pool');p.write_text(s)
# The extraction follows pending vs answered invite states, keeping every new function bounded.
p=Path('src/adapters/http/pages/team_settings.rs');s=p.read_text();start=s.index('fn invite_details(');end=s.index('\npub fn invite_pane(',start);chunk=s[start:end];a=chunk.index('        format!(\n');b=chunk.index('\n    } else if status',a);pending=chunk[a:b];c=chunk.index('        let delete_button =',b);closed=chunk[c:chunk.rfind('\n    }\n}')]
prefix=chunk[:chunk.index('    // An invite')];prefix=prefix.replace('fn invite_details','fn pending_invite_details').replace('    let status = InviteStatus::parse(&invite.status);\n','');pendingfn=prefix+pending+'\n}\n'
closedprefix='''fn answered_invite_details(pane: &InvitePane<'_>) -> String {
    let invite = pane.invite;
    let status = InviteStatus::parse(&invite.status);
    let invite_endpoint = team_endpoint(pane.company.id, &format!("/invites/{}", invite.id));
    let close_endpoint = team_endpoint(pane.company.id, "/close");
    let cleared_url = team_url(pane.company.id, TeamSelection::None);
''';closedfn=closedprefix+closed+'\n}\n'
router='''fn invite_details(pane: &InvitePane<'_>) -> String {
    if InviteStatus::parse(&pane.invite.status) != InviteStatus::Pending {
        return answered_invite_details(pane);
    }
    if pane.role.manages() { return pending_invite_details(pane); }
    format!("<p>Pending invitation · {}</p>", pane.invite.role.label())
}
''';s=s[:start]+pendingfn+'\n'+closedfn+'\n'+router+s[end:];s=s.replace('email = escape_html_text(email),','email = escape_html_attr(email),').replace('email = escape_html_text(&invite.email),','email = escape_html_attr(&invite.email),');p.write_text(s)
p=Path('src/adapters/http/pages/companies.rs');s=p.read_text().replace('if !actor.membership.is_owner() {\n        return company_invite_row_fragment','if !actor.membership.is_owner() || invite.status != "pending" {\n        return company_invite_row_fragment');s=s.replace('email = escape_html_text(&invite.email),','email = escape_html_attr(&invite.email),');p.write_text(s)
# Existing workflow authority-transfer fixtures must promote the new owner to level 9 too.
p=Path('src/adapters/persistence/workflow/message_authority_tests.rs');s=p.read_text().replace("UPDATE company_members SET role='owner' WHERE", "UPDATE company_members SET role='owner', access_level=9 WHERE");p.write_text(s)
