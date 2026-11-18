//! One sorted account union precedes company ownership, including real message user FKs.
use super::super::approval_accounts::ActionApprovalAccounts;
use super::super::row_locks::RowLocks;
use super::*;
use crate::application::workflow::decisions::DecisionNotices;
use crate::application::workflow::message::{DestinationKind, MessagePlan};

/// The live caller carries the chosen lock policy through its subsequent run ownership.
#[derive(Clone, Copy)]
pub(in crate::adapters::persistence::workflow) struct AccountPrefix {
    pub(in crate::adapters::persistence::workflow) actor: Option<WorkflowActor>,
    pub(in crate::adapters::persistence::workflow) locks: RowLocks,
}

impl AccountPrefix {
    pub(in crate::adapters::persistence::workflow) fn error(self, error: AppError) -> AppError {
        match self.locks {
            RowLocks::NoWait => super::super::native_mcp::busy(error),
            RowLocks::Wait => error,
        }
    }
}

#[derive(Clone, Copy)]
enum Purpose<'a> {
    Effect,
    Reply,
    Dispatch {
        request: &'a ActionDispatchRequest,
        notices: Option<&'a DecisionNotices>,
    },
    Message {
        request: &'a ActionDispatchRequest,
        plan: &'a MessagePlan,
        notices: Option<&'a DecisionNotices>,
    },
}

#[derive(PartialEq, Eq)]
pub(in crate::adapters::persistence::workflow) struct ActorAccountHints {
    actor: Uuid,
    requester: Option<WorkflowActor>,
    reviewer: Option<WorkflowActor>,
}

impl ActorAccountHints {
    pub(in crate::adapters::persistence::workflow) fn actor(&self) -> AppResult<WorkflowActor> {
        WorkflowActor::authenticated(self.actor)
    }

    pub(in crate::adapters::persistence::workflow) fn users(
        &self,
    ) -> impl Iterator<Item = Uuid> + '_ {
        std::iter::once(self.actor)
            .chain(self.requester.map(WorkflowActor::user_id))
            .chain(self.reviewer.map(WorkflowActor::user_id))
    }
}

#[derive(PartialEq, Eq)]
enum Source {
    NotApplicable,
    Reply(super::super::approval_accounts::ApprovalAccounts),
    Dispatch {
        approval: ActionApprovalAccounts,
        escalation: Option<super::super::escalations::CommandTarget>,
    },
    Message {
        recipients: MessageSource,
        approval: ActionApprovalAccounts,
    },
}

#[derive(PartialEq, Eq)]
enum MessageSource {
    FreshMessage(Vec<Uuid>),
    RecordedMessage,
}

impl Source {
    fn users(&self) -> impl Iterator<Item = Uuid> + '_ {
        let (recipients, approval) = match self {
            Self::NotApplicable | Self::Reply(_) => (&[][..], None),
            Self::Dispatch { approval, .. } => (&[][..], Some(approval)),
            Self::Message {
                recipients,
                approval,
            } => (
                match recipients {
                    MessageSource::FreshMessage(users) => users.as_slice(),
                    MessageSource::RecordedMessage => &[],
                },
                Some(approval),
            ),
        };
        let reply = match self {
            Self::Reply(approval) => Some(approval),
            _ => None,
        };
        let escalation = match self {
            Self::Dispatch {
                escalation: Some(target),
                ..
            } => Some(target.account),
            _ => None,
        };
        recipients
            .iter()
            .copied()
            .chain(approval.into_iter().flat_map(|hint| hint.users()))
            .chain(reply.into_iter().flat_map(|hint| hint.users()))
            .chain(escalation)
    }

    fn recorded(&self) -> bool {
        matches!(
            self,
            Self::Dispatch {
                approval: ActionApprovalAccounts::Recorded,
                ..
            } | Self::Message {
                recipients: MessageSource::RecordedMessage,
                ..
            }
        )
    }
}

pub(in crate::adapters::persistence::workflow) fn prelock_accounts_on(
    db: &mut PgConnection,
    scope: ActionScope,
    reviewer: Option<WorkflowActor>,
) -> impl std::future::Future<Output = AppResult<Option<WorkflowActor>>> + '_ {
    prelock(
        db,
        scope,
        reviewer,
        Purpose::Effect,
        super::super::row_locks::RowLocks::Wait,
    )
}

pub(in crate::adapters::persistence::workflow) fn prelock_accounts_with_locks_on(
    db: &mut PgConnection,
    scope: ActionScope,
    reviewer: Option<WorkflowActor>,
    locks: super::super::row_locks::RowLocks,
) -> impl std::future::Future<Output = AppResult<Option<WorkflowActor>>> + '_ {
    prelock(db, scope, reviewer, Purpose::Effect, locks)
}

/// Final-response owners may actually create or consume the frozen reply approval.
pub(in crate::adapters::persistence::workflow) fn prelock_reply_on(
    db: &mut PgConnection,
    scope: ActionScope,
    reviewer: Option<WorkflowActor>,
) -> impl std::future::Future<Output = AppResult<Option<WorkflowActor>>> + '_ {
    prelock(
        db,
        scope,
        reviewer,
        Purpose::Reply,
        super::super::row_locks::RowLocks::Wait,
    )
}

pub(in crate::adapters::persistence::workflow) fn prelock_message_on<'a>(
    db: &'a mut PgConnection,
    request: &'a ActionDispatchRequest,
    plan: &'a MessagePlan,
    notices: Option<&'a DecisionNotices>,
) -> impl std::future::Future<Output = AppResult<Option<WorkflowActor>>> + 'a {
    prelock(
        db,
        request.scope(),
        None,
        Purpose::Message {
            request,
            plan,
            notices,
        },
        super::super::row_locks::RowLocks::Wait,
    )
}

pub(in crate::adapters::persistence::workflow) fn prelock_dispatch_on<'a>(
    db: &'a mut PgConnection,
    request: &'a ActionDispatchRequest,
    notices: Option<&'a DecisionNotices>,
) -> impl std::future::Future<Output = AppResult<AccountPrefix>> + 'a {
    live_prefix(
        db,
        request.scope(),
        None,
        Purpose::Dispatch { request, notices },
    )
}

pub(in crate::adapters::persistence::workflow) fn prelock_live_on(
    db: &mut PgConnection,
    scope: ActionScope,
    reviewer: Option<WorkflowActor>,
) -> impl std::future::Future<Output = AppResult<AccountPrefix>> + '_ {
    live_prefix(db, scope, reviewer, Purpose::Effect)
}

pub(in crate::adapters::persistence::workflow) async fn prelock_native_on(
    db: &mut PgConnection,
    scope: ActionScope,
) -> AppResult<Option<AccountPrefix>> {
    let Some(native) = super::super::native_mcp::account_hints_on(db, scope).await? else {
        return Ok(None);
    };
    current_prefix(db, scope, None, Purpose::Effect, Some(native))
        .await
        .map(Some)
}

async fn live_prefix(
    db: &mut PgConnection,
    scope: ActionScope,
    reviewer: Option<WorkflowActor>,
    purpose: Purpose<'_>,
) -> AppResult<AccountPrefix> {
    let native = super::super::native_mcp::account_hints_on(db, scope).await?;
    current_prefix(db, scope, reviewer, purpose, native).await
}

async fn current_prefix(
    db: &mut PgConnection,
    scope: ActionScope,
    reviewer: Option<WorkflowActor>,
    purpose: Purpose<'_>,
    native: Option<super::super::native_mcp::NativeAccounts>,
) -> AppResult<AccountPrefix> {
    let prefix = AccountPrefix {
        actor: None,
        locks: if native.is_some() {
            RowLocks::NoWait
        } else {
            RowLocks::Wait
        },
    };
    // Box the full account/source union seam so native preparation does not grow the poll stack.
    let actor = Box::pin(prelock_with_native(
        db,
        scope,
        reviewer,
        purpose,
        prefix.locks,
        native,
    ))
    .await
    .map_err(|error| prefix.error(error))?;
    if matches!(prefix.locks, RowLocks::NoWait) {
        actor.ok_or_else(missing)?;
        // Shared fence readers take this run before job/attempt ownership. Pin it NOWAIT
        // here so an earlier generic fence helper cannot introduce a waiting native edge.
        sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE NOWAIT",
        )
        .bind(scope.company.as_uuid())
        .bind(scope.run.as_uuid())
        .fetch_optional(db)
        .await
        .map_err(|error| prefix.error(error.into()))?
        .ok_or_else(missing)?;
    }
    Ok(AccountPrefix { actor, ..prefix })
}

pub(in crate::adapters::persistence::workflow) async fn actor_hints_on(
    db: &mut PgConnection,
    scope: ActionScope,
    reviewer: Option<WorkflowActor>,
) -> AppResult<Option<ActorAccountHints>> {
    let actor: Option<Uuid> =
        sqlx::query_scalar("SELECT actor_id FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(scope.company.as_uuid())
            .bind(scope.run.as_uuid())
            .fetch_optional(&mut *db)
            .await?;
    let Some(actor) = actor else { return Ok(None) };
    Ok(Some(ActorAccountHints {
        actor,
        reviewer,
        requester: super::super::provisioning::requester_on(db, scope).await?,
    }))
}

async fn source_on(
    db: &mut PgConnection,
    scope: ActionScope,
    purpose: Purpose<'_>,
) -> AppResult<Source> {
    match purpose {
        Purpose::Effect => Ok(Source::NotApplicable),
        Purpose::Reply => Ok(Source::Reply(
            super::super::approval_accounts::load_on(db, scope).await?,
        )),
        Purpose::Dispatch { request, notices } => {
            let approval = super::super::approval_accounts::action_on(db, request, notices).await?;
            let escalation = if matches!(approval, ActionApprovalAccounts::Recorded) {
                None
            } else {
                super::super::escalation_tools::target_hint_on(db, request).await?
            };
            Ok(Source::Dispatch {
                approval,
                escalation,
            })
        }
        Purpose::Message {
            request,
            plan,
            notices,
        } => {
            let recipients = message_source_on(db, request, plan).await?;
            let approval = match recipients {
                MessageSource::RecordedMessage => ActionApprovalAccounts::Recorded,
                _ => super::super::approval_accounts::action_on(db, request, notices).await?,
            };
            Ok(Source::Message {
                recipients,
                approval,
            })
        }
    }
}

async fn message_source_on(
    db: &mut PgConnection,
    request: &ActionDispatchRequest,
    plan: &MessagePlan,
) -> AppResult<MessageSource> {
    let recorded: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_action_dispatches WHERE company_id=$1 AND invocation_id=$2)")
        .bind(request.scope().company.as_uuid()).bind(request.subject.invocation.as_uuid()).fetch_one(db).await?;
    if recorded {
        return Ok(MessageSource::RecordedMessage);
    }
    if plan.destinations.len() > registry::MAX_MESSAGE_DESTINATIONS {
        return Err(invalid());
    }
    // These are the typed values written to destination_user_id, not UUIDs guessed from JSON
    // or the route-check variant. The later authoritative route/subject checks still decide.
    let users = plan
        .destinations
        .iter()
        .filter(|destination| matches!(destination.kind, DestinationKind::User))
        .map(|destination| destination.destination_id)
        .collect();
    Ok(MessageSource::FreshMessage(users))
}

fn prelock<'a>(
    db: &'a mut PgConnection,
    scope: ActionScope,
    reviewer: Option<WorkflowActor>,
    purpose: Purpose<'a>,
    locks: RowLocks,
) -> impl std::future::Future<Output = AppResult<Option<WorkflowActor>>> + 'a {
    prelock_with_native(db, scope, reviewer, purpose, locks, None)
}

async fn prelock_with_native(
    db: &mut PgConnection,
    scope: ActionScope,
    reviewer: Option<WorkflowActor>,
    purpose: Purpose<'_>,
    locks: RowLocks,
    native: Option<super::super::native_mcp::NativeAccounts>,
) -> AppResult<Option<WorkflowActor>> {
    let Some(hints) = actor_hints_on(db, scope, reviewer).await? else {
        return Ok(None);
    };
    let mut source = source_on(db, scope, purpose).await?;
    let mut accounts = hints
        .users()
        .chain(source.users())
        .chain(native.iter().flat_map(|native| native.users()))
        .collect();
    let locked =
        super::super::reviewer_accounts::lock_users_with_locks_on(db, &mut accounts, locks).await?;
    if locked != accounts {
        // A concurrent publication can turn this into observation while a destination account
        // retires. Observation needs no historical recipient; every live base account must
        // already be locked. Never acquire a replacement account after company ownership.
        source = source_on(db, scope, purpose).await?;
        if !source.recorded()
            || !hints
                .users()
                .all(|user| locked.binary_search(&user).is_ok())
            || native
                .iter()
                .flat_map(|native| native.users())
                .any(|user| locked.binary_search(&user).is_err())
        {
            return Err(missing());
        }
    }
    let actor = hints.actor()?;
    match super::super::management::authorize_run_with_locks(
        db,
        scope.company,
        scope.run,
        actor,
        locks,
    )
    .await
    {
        Ok(_) => {
            if actor_hints_on(db, scope, reviewer).await?.as_ref() != Some(&hints) {
                return Err(missing());
            }
            if native.is_some()
                && super::super::native_mcp::account_hints_on(db, scope).await? != native
            {
                return Err(missing());
            }
            let current = source_on(db, scope, purpose).await?;
            if source != current && !current.recorded() {
                return Err(conflict());
            }
            Ok(Some(actor))
        }
        Err(AppError::NotFound(_)) => Ok(None),
        Err(error) => Err(error),
    }
}
