//! Use cases receive their small settings and purpose-specific ports.
use super::*;
/// Shared production construction also used by native key-rotation tests.
pub(crate) fn user_use_cases(
    config: &AppConfig,
    persistence: Arc<crate::adapters::persistence::PostgresPersistence>,
    confirmation: EmailConfirmation,
) -> UserUseCases {
    UserUseCases::new(
        Arc::new(argon2_password_hasher()),
        persistence,
        config.auth_attempt_secret.clone(),
    )
    .with_email_confirmation(confirmation)
}
pub(super) struct UseCases {
    pub(super) user: Arc<UserUseCases>,
    pub(super) company: Arc<CompanyUseCases>,
    pub(super) invites: Arc<CompanyInviteUseCases>,
    pub(super) resend: Arc<CompanyResendApiUseCases>,
    pub(super) channel: Arc<ChannelUseCases>,
    pub(super) department: Arc<crate::use_cases::department::DepartmentUseCases>,
    pub(super) agent: Arc<AgentUseCases>,
    pub(super) skill: Arc<SkillUseCases>,
    pub(super) onboarding: Arc<crate::use_cases::onboarding::OnboardingUseCases>,
    pub(super) schedules: Arc<ScheduleUseCases>,
    pub(super) attention: Arc<crate::application::attention::AttentionUseCases>,
}
fn agents(
    p: &Arc<crate::adapters::persistence::PostgresPersistence>,
    workflow: &crate::infra::workflow::WorkflowServices,
) -> anyhow::Result<Arc<AgentUseCases>> {
    let classifier: Arc<dyn TextClassifier> =
        Arc::new(crate::adapters::harness::rig::RigTextClassifier::new(
            crate::adapters::harness::rig::providers::ProviderRegistry::standard()?,
        ));
    Ok(Arc::new(
        AgentUseCases::new(p.clone(), p.clone(), p.clone())
            .with_company_agents(workflow.company_agents())
            .with_prompt_classifier(classifier),
    ))
}
pub(super) fn build(
    f: &foundation::Foundation,
    workflow: &crate::infra::workflow::WorkflowServices,
) -> anyhow::Result<UseCases> {
    let p = &f.persistence;
    let user = Arc::new(user_use_cases(
        &f.config,
        p.clone(),
        EmailConfirmation {
            registrations: p.clone(),
            account_changes: p.clone(),
            codes: Arc::new(SmtpConfirmationSender::new(
                f.config.clone(),
                f.mail_transport.clone(),
            )),
            transport_available: f.mail_available,
            allow_unverified_without_smtp: allow_unverified_email_without_smtp_from_env(),
        },
    ));
    let agent = agents(p, workflow)?;
    Ok(UseCases {
        user,
        company: Arc::new(CompanyUseCases::new(p.clone())),
        invites: Arc::new(CompanyInviteUseCases::new(p.clone(), p.clone())),
        resend: Arc::new(CompanyResendApiUseCases::new(p.clone())),
        channel: Arc::new(ChannelUseCases::new(
            p.clone(),
            p.clone(),
            p.clone(),
            f.config.spam_scanning(),
        )),
        department: Arc::new(crate::use_cases::department::DepartmentUseCases::new(
            p.clone(),
            p.clone(),
        )),
        onboarding: Arc::new(crate::use_cases::onboarding::OnboardingUseCases::new(
            p.clone(),
            p.clone(),
            workflow.onboarding_persistence(),
            agent.clone(),
        )),
        agent,
        skill: Arc::new(SkillUseCases::new(p.clone(), p.clone(), p.clone())),
        schedules: Arc::new(ScheduleUseCases::new(
            p.clone(),
            workflow.authorization(),
            workflow.schedule_run_now(),
        )),
        attention: Arc::new(crate::application::attention::AttentionUseCases::new(
            p.clone(),
            p.clone(),
            p.clone(),
            p.clone(),
            f.monitoring.clone(),
        )),
    })
}
