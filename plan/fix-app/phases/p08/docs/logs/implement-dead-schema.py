from pathlib import Path
import re

def edit(name, old, new, count=1):
 p=Path(name);s=p.read_text();assert s.count(old)==count,(name,old,s.count(old),count);p.write_text(s.replace(old,new))

schema='migrations/20260817000000_init_schema.sql'
p=Path(schema);s=p.read_text()
def sub(old,new,count=1):
 global s
 assert s.count(old)==count,(old,s.count(old),count)
 s=s.replace(old,new)
sub('    max_retries integer DEFAULT 3 NOT NULL,\n    last_error text,\n','    max_retries integer DEFAULT 3 NOT NULL,\n')
sub("audience text DEFAULT 'legacy_unclassified'::text NOT NULL",'audience text NOT NULL')
sub(", 'legacy_unclassified'::text",'')
sub("ARRAY['reply'::text, 'mirror'::text, 'outreach'::text, 'notification'::text, 'workflow_send'::text, 'workflow_reply'::text]", "ARRAY['notification'::text, 'workflow_send'::text, 'workflow_reply'::text]")
sub("ARRAY['observed'::text, 'verified'::text, 'disabled'::text]", "ARRAY['observed'::text, 'verified'::text]")
sub('    delivery_part_id uuid,\n','')
start=s.index('--\n-- Name: external_messages_delivery_part_idx;');end=s.index('\n\n--\n',start+4);s=s[:start]+s[end+2:]
start=s.index('--\n-- Name: external_messages external_messages_delivery_part_fk;');end=s.index('\n\n--\n',start+4);s=s[:start]+s[end+2:]
a=s.index('CREATE TABLE public.task_attempts (');b=s.index('\n);',a)
part=s[a:b];part=part.replace('    machine_id text NOT NULL,\n','').replace('    machine_region text,\n','').replace('    CONSTRAINT task_attempts_machine_id_check CHECK ((length(TRIM(BOTH FROM machine_id)) > 0)),\n','');s=s[:a]+part+s[b:]
sub('    final_output_consumed_at timestamp with time zone,\n','')
sub(' AND final_output_consumed_at IS NULL','')
sub('    opportunity_until timestamp with time zone,\n    registered_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL','    opportunity_until timestamp with time zone')
p.write_text(s)

edit('src/domain/entities/message.rs','message with another thread must never make private content public. `LegacyUnclassified` is a\n/// fail-closed migration state and is deliberately not externally deliverable.','message with another thread must never make private content public.')
for old in ('    LegacyUnclassified,\n','            Self::LegacyUnclassified => "legacy_unclassified",\n','            "legacy_unclassified" => Ok(Self::LegacyUnclassified),\n'):edit('src/domain/entities/message.rs',old,'')
edit('src/application/workflow/registry/schemas.rs','"external_conversation","internal_only","legacy_unclassified"','"external_conversation","internal_only"')
for old in ('    Disabled,\n','            Self::Disabled => "disabled",\n','            "disabled" => Ok(Self::Disabled),\n'):edit('src/domain/entities/participant.rs',old,'')
p=Path('src/application/use_cases/participant/test_support.rs');s=p.read_text();a=s.index('        if known\n',s.index('async fn access_context_for_identity'));b=s.index('        // Test fixtures state ACL principals',a);s=s[:a]+s[b:];p.write_text(s)
edit('src/domain/entities/transport/delivery.rs','''        /// The answer to something that arrived on this interface.
        Reply => "reply",
        /// The same message exposed through another eligible interface.
        Mirror => "mirror",
        /// The agent opening a conversation with a third party.
        Outreach => "outreach",
''','')
edit('src/domain/entities/transport/delivery.rs','Self::Outreach | Self::WorkflowSend => true,','Self::WorkflowSend => true,')
edit('src/domain/entities/transport/delivery.rs','Self::Reply | Self::Mirror | Self::Notification | Self::WorkflowReply => false,','Self::Notification | Self::WorkflowReply => false,')
for old in ('            Self::Reply => "Reply",\n','            Self::Mirror => "Mirror",\n','            Self::Outreach => "Outreach",\n'):edit('src/domain/entities/transport/delivery.rs',old,'')
for p in Path('src').rglob('*.rs'):
 s=p.read_text();n=s.replace('DeliveryPurpose::Reply','DeliveryPurpose::WorkflowReply').replace('DeliveryPurpose::Mirror','DeliveryPurpose::WorkflowReply').replace('DeliveryPurpose::Outreach','DeliveryPurpose::WorkflowSend')
 if n!=s:p.write_text(n)
edit('src/adapters/persistence/delivery/enqueue.rs','Internal-only and unclassified messages cannot be delivered externally.','Internal-only messages cannot be delivered externally.')
for name in ('src/domain/entities/dashboard.rs','src/adapters/http/pages/dashboard.rs','src/adapters/persistence/dashboard.rs'):
 p=Path(name);s=p.read_text();assert 'last_error' in s;s=s.replace('last_error','workflow_failure_code');p.write_text(s)
edit('src/adapters/persistence/dashboard.rs','           task.workflow_failure_code,','''           (SELECT attempt.workflow_failure_code
              FROM task_attempts AS attempt
             WHERE attempt.task_id = task.id
             ORDER BY attempt.attempt_number DESC, attempt.started_at DESC, attempt.id DESC
             LIMIT 1) AS workflow_failure_code,''')
edit('src/adapters/http/pages/dashboard.rs','''    // The error is why the row is here at all, so it earns a line — truncated, because a stack of
    // provider errors would push every other row off the panel.''','''    // Show the latest attempt's failure code when this job needs attention.''')
# Remove only the dead machine marker in actual attempt SQL, retaining every ownership predicate.
replacements={
'src/adapters/persistence/workflow/lease_claim.rs':[(',worker_id,machine_id,started_at)',',worker_id,started_at)'),("'processing',$4,'workflow',clock_timestamp()","'processing',$4,clock_timestamp()")],
'src/adapters/persistence/workflow/action_reconciliation_claim_sql_deferred_tests.rs':[(',worker_id,machine_id)',',worker_id)'),("'processing',$4,$5,'deferred-refusal-attack'","'processing',$4,$5"),(" AND attempt.machine_id='deferred-refusal-attack'",'')],
'src/adapters/persistence/workflow/activation_tests.rs':[(', worker_id, machine_id)',', worker_id)'),(", gen_random_uuid(), 'activation-test')",", gen_random_uuid())")],
'src/adapters/persistence/workflow/job_association_tests.rs':[(', worker_id, machine_id)',', worker_id)'),(", gen_random_uuid(), 'association-test')",", gen_random_uuid())")],
'src/adapters/persistence/workflow/job_projection_tests.rs':[(',worker_id,machine_id,started_at,finished_at)',',worker_id,started_at,finished_at)'),("gen_random_uuid(),'projection',CURRENT_TIMESTAMP","gen_random_uuid(),CURRENT_TIMESTAMP")],
}
for name,pairs in replacements.items():
 for old,new in pairs:edit(name,old,new)
edit('src/adapters/persistence/workflow/harness_owner_tests.rs','        ("final_output_consumed_at", json!(chrono::Utc::now())),\n','')
edit('src/adapters/persistence/workflow/fairness_tests.rs','''    sqlx::query(
        "UPDATE workflow_dispatch_demand SET registered_at=clock_timestamp()-interval '2 minutes'",
    )
    .execute(p.pool())
    .await
    .unwrap();
''','')
edit('src/adapters/persistence/workflow/fairness_worker_tests.rs','''            sqlx::query("UPDATE workflow_dispatch_demand SET registered_at=clock_timestamp()-interval '2 minutes'")
                .execute(p.pool()).await.unwrap();
''','')
# The former legacy-row test becomes an actual schema refusal test: no migration state remains.
p=Path('src/adapters/persistence/thread/tests.rs');s=p.read_text();a=s.index('async fn legacy_unclassified_messages_remain_readable_but_not_deliverable()');b=s.index('\n}',a)+2
s=s[:a]+'''async fn schema_constraint_messages_require_declared_audience() {
    let fixture = Fixture::new("declared_audience").await.expect("native fixture");
    let stored = fixture.persistence.create_message(&inbound_email(
        fixture.thread.id,
        email_metadata(&format!("<audience-{}@partner.test>", fixture.suffix)),
        "Explicit audience",
    )).await.unwrap();
    for (expression, expected) in [("'legacy_unclassified'", "23514"), ("DEFAULT", "23502")] {
        let error = sqlx::query(&format!("UPDATE messages SET audience = {expression} WHERE id = $1"))
            .bind(stored.canonical_id.as_uuid()).execute(&fixture.pool).await.unwrap_err();
        assert_eq!(error.as_database_error().and_then(|error| error.code()).as_deref(), Some(expected));
    }
    let read = fixture.persistence.get_thread_message(fixture.thread.id, stored.canonical_id)
        .await.unwrap().unwrap();
    assert_eq!(read.audience, MessageAudience::ExternalConversation);
    fixture.cleanup().await;
}'''+s[b:];p.write_text(s)
print('Dead schema/value changes applied')
