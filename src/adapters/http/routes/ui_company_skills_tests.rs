//! B1-F1: the company Skills tab renders for anyone who manages a unit (Plan07 decision a), read
//! only for a member who manages none.
use super::super::authorization_test_support::Fixture;
use super::*;

fn workspace(f: &Fixture, user: &crate::entities::user::User) -> Workspace {
    let p = f.persistence.clone();
    Workspace {
        company_use_cases: f.companies.clone(),
        channel_use_cases: f.channels.clone(),
        agent_use_cases: f.agents.clone(),
        invite_use_cases: f.invites.clone(),
        resend_api_use_cases: Arc::new(CompanyResendApiUseCases::new(p.clone())),
        user_use_cases: f.users.clone(),
        skill_use_cases: Arc::new(SkillUseCases::new(p.clone(), p.clone(), p.clone())),
        department_use_cases: f.departments(),
        knowledge: Arc::new(crate::application::knowledge::KnowledgeLibrary::new(
            p,
            Arc::new(
                crate::application::knowledge::test_support::DeterministicTextEmbedder::default(),
            ),
        )),
        config: f.config.clone(),
        user_id: user.id,
    }
}

#[tokio::test]
async fn unit_resource_skills_tab_renders_editable_for_unit_managers_and_read_only_for_members() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let d = f.delegation().await;
    let query = || {
        Query(WorkspaceQuery {
            company_id: Some(f.company.id),
            tab: Some("skills".into()),
            ..WorkspaceQuery::default()
        })
    };
    for user in [&f.owner, &d.admin, &d.lead, &d.sibling] {
        let html = companies_page(workspace(&f, user), query())
            .await
            .expect("a unit manager opens the Skills tab")
            .0;
        assert!(
            html.contains("id=\"skill-library-section\""),
            "{}",
            user.username
        );
        assert!(
            html.contains("name=\"unit_id\""),
            "the new skill names its unit"
        );
    }
    let html = companies_page(workspace(&f, &f.member), query())
        .await
        .expect("a member still reads the pane")
        .0;
    assert!(html.contains("Only a department's managers can edit its skills."));
    assert!(!html.contains("id=\"skill-library-section\""));
}
