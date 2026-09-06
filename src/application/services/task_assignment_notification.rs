//! Compose the narrowly scoped email sent when work is assigned to another human.

use crate::{
    app_error::AppResult,
    entities::{
        correlation::CorrelationId,
        task::{TaskOwner, TaskOwnershipCommand, TaskOwnershipEvent},
        transport::DeliveryPurpose,
        value_objects::EmailAddress,
    },
    infra::config::AppConfig,
    task_queue::TaskPersistence,
    transport::{
        CanonicalContent, DeliveryComposer, DeliveryContext, EmailDeliveryContext, EmailThreading,
        StandaloneDeliveryRequest,
    },
};

pub struct AssignmentNotificationContext<'a> {
    pub company_name: &'a str,
    pub channel_id: uuid::Uuid,
    pub thread_id: Option<uuid::Uuid>,
    pub correlation_id: CorrelationId,
}

pub async fn change_ownership_with_assignment_notification(
    persistence: &dyn TaskPersistence,
    deliveries: &DeliveryComposer,
    config: &AppConfig,
    context: AssignmentNotificationContext<'_>,
    command: TaskOwnershipCommand,
) -> AppResult<TaskOwnershipEvent> {
    let notification =
        assignment_notification(persistence, deliveries, config, &context, &command).await?;
    persistence
        .change_task_ownership_with_notification(command, notification)
        .await
}

async fn assignment_notification(
    persistence: &dyn TaskPersistence,
    deliveries: &DeliveryComposer,
    config: &AppConfig,
    context: &AssignmentNotificationContext<'_>,
    command: &TaskOwnershipCommand,
) -> AppResult<Option<crate::transport::NewStandaloneDelivery>> {
    let Some(recipient_principal) = assignment_recipient(command) else {
        return Ok(None);
    };
    let Some(recipient) = persistence
        .assignment_notification_recipient(command.company_id, recipient_principal)
        .await?
    else {
        return Ok(None);
    };
    let subject = format!("Task assigned in {}", context.company_name);
    let link = match context.thread_id {
        Some(thread_id) => format!(
            "{}/ui?company_id={}&channel_id={}&thread_id={}",
            config.public_base_url(),
            command.company_id,
            context.channel_id,
            thread_id
        ),
        None => format!(
            "{}/ui/tasks?company_id={}&task_id={}&view=list",
            config.public_base_url(),
            command.company_id,
            command.task_id
        ),
    };
    let body = format!(
        "A task in {} was assigned to you.\n\nOpen it securely: {}",
        context.company_name, link
    );
    let content = CanonicalContent::parse(subject, body)?;
    let assigned_version = command.expected_version.checked_add(1).ok_or_else(|| {
        crate::app_error::AppError::Conflict("Ownership version exhausted.".into())
    })?;
    let source_key = format!(
        "task:{}:assignment:{}:user:{}",
        command.task_id, assigned_version, recipient.user_id
    );
    let delivery = deliveries.compose_standalone(StandaloneDeliveryRequest {
        correlation_id: context.correlation_id,
        purpose: DeliveryPurpose::Notification,
        source_key,
        content: &content,
        context: DeliveryContext::Email(EmailDeliveryContext {
            from: EmailAddress::from(format!("notifications@{}", config.app_domain_name)),
            from_name: Some("Mail Agents".into()),
            recipient_to: recipient.email,
            recipients_cc: Vec::new(),
            threading: EmailThreading::Standalone,
            relay: None,
        }),
    })?;
    Ok(Some(delivery))
}

fn assignment_recipient(
    command: &TaskOwnershipCommand,
) -> Option<crate::entities::transport::PrincipalId> {
    match command.new_owner {
        TaskOwner::Human(principal_id) if principal_id != command.actor.principal_id => {
            Some(principal_id)
        }
        TaskOwner::Human(_) | TaskOwner::Agent(_) | TaskOwner::Unassigned => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{
        task::{
            TaskOwnershipActor, TaskOwnershipAuthority, TaskOwnershipOperation, TaskOwnershipReason,
        },
        transport::PrincipalId,
    };

    fn command(actor: PrincipalId, owner: TaskOwner) -> TaskOwnershipCommand {
        TaskOwnershipCommand {
            task_id: uuid::Uuid::new_v4(),
            company_id: uuid::Uuid::new_v4(),
            command_id: uuid::Uuid::new_v4(),
            expected_version: 1,
            actor: TaskOwnershipActor {
                principal_id: actor,
                authority: TaskOwnershipAuthority::Manager,
            },
            operation: TaskOwnershipOperation::Assign,
            new_owner: owner,
            reason: TaskOwnershipReason::ManualAssignment,
            reason_detail: None,
            handoff_instruction: None,
        }
    }

    #[test]
    fn assignment_mail_targets_only_another_human() {
        let actor = PrincipalId::random();
        let teammate = PrincipalId::random();

        assert_eq!(
            assignment_recipient(&command(actor, TaskOwner::Human(teammate))),
            Some(teammate)
        );
        assert_eq!(
            assignment_recipient(&command(actor, TaskOwner::Human(actor))),
            None,
            "self-assignment is silent"
        );
        assert_eq!(
            assignment_recipient(&command(actor, TaskOwner::Agent(teammate))),
            None,
            "agent assignments do not create human email"
        );
    }
}
