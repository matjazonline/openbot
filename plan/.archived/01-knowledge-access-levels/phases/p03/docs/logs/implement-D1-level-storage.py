from pathlib import Path
import re
p=Path('src/domain/entities/access_level.rs');s=p.read_text();s+='''
/// The least cleared reader determines the clearance of a shared audience.
pub fn audience_level(readers: impl IntoIterator<Item = AccessLevel>) -> AccessLevel {
    AccessLevel::lowest(readers)
}

/// A release never lowers the classification of the content it carries.
pub const fn may_release(content: AccessLevel, destination: AccessLevel) -> bool {
    content.as_i16() <= destination.as_i16()
}

/// Immutable admission ceiling and the monotone classification of data already consumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RunKnowledge {
    knowledge_ceiling: AccessLevel,
    content_level: AccessLevel,
}
impl RunKnowledge {
    pub fn new(knowledge_ceiling: AccessLevel, content_level: AccessLevel) -> Result<Self, &'static str> {
        if content_level > knowledge_ceiling {
            return Err("Run content exceeds its knowledge ceiling");
        }
        Ok(Self { knowledge_ceiling, content_level })
    }
    pub const fn ceiling(self) -> AccessLevel { self.knowledge_ceiling }
    pub const fn content(self) -> AccessLevel { self.content_level }
}

#[cfg(test)]
#[path = "knowledge_level_tests.rs"]
mod knowledge_tests;
''';p.write_text(s)
p=Path('migrations/20260817000000_init_schema.sql');s=p.read_text()
def columns(table, new):
 global s
 marker=f'CREATE TABLE public.{table} (\n';assert s.count(marker)==1,table;s=s.replace(marker,marker+new,1)
def constraint(table, text):
 global s
 start=s.index(f'CREATE TABLE public.{table} (\n');end=s.index('\n);',start);part=s[start:end]
 part+=',\n'+text.rstrip(',\n');s=s[:start]+part+s[end:]
columns('workflow_runs','    knowledge_ceiling public.access_level NOT NULL,\n    content_level public.access_level DEFAULT 0 NOT NULL,\n')
constraint('workflow_runs','    CONSTRAINT workflow_runs_knowledge_bounds CHECK (content_level <= knowledge_ceiling)')
columns('messages','    access_level public.access_level DEFAULT 0 NOT NULL,\n')
constraint('messages',"    CONSTRAINT messages_human_public CHECK (role <> 'human' OR access_level = 0)")
for table in ('workflow_http_connections','company_mcp_connections'):
 columns(table,'    access_level public.access_level DEFAULT 0 NOT NULL,\n')
columns('workflow_action_intents','    content_level_at_freeze public.access_level NOT NULL,\n    destination_level_at_freeze public.access_level NOT NULL,\n')
constraint('workflow_action_intents','    CONSTRAINT workflow_action_knowledge_bounds CHECK (content_level_at_freeze <= destination_level_at_freeze)')
# The snapshot remains immutable except for the explicitly monotone content watermark.
start=s.index('CREATE FUNCTION public.guard_workflow_run_snapshot()');end=s.index('CREATE FUNCTION public.guard_workflow_publication_snapshot()',start);part=s[start:end]
assert part.count("'terminal_wakeup_sequence','revision','finished_at']")==2
part=part.replace("'terminal_wakeup_sequence','revision','finished_at']","'terminal_wakeup_sequence','revision','finished_at','content_level']")
part=part.replace('    created_at, parent_run_id, parent_execution_id, rerun_of_run_id, delivery_policy\n','    created_at, parent_run_id, parent_execution_id, rerun_of_run_id, delivery_policy, knowledge_ceiling\n')
s=s[:start]+part+s[end:]
s+='''

-- The watermark and terminal child facts cannot be rewritten by a later checkpoint.
CREATE FUNCTION public.guard_workflow_run_knowledge() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.knowledge_ceiling IS DISTINCT FROM OLD.knowledge_ceiling
       OR NEW.content_level < OLD.content_level
       OR (OLD.state IN ('succeeded','failed','cancelled') AND NEW.content_level IS DISTINCT FROM OLD.content_level)
    THEN
        RAISE EXCEPTION 'workflow knowledge facts cannot be lowered or rewritten'
            USING ERRCODE='23514', CONSTRAINT='workflow_guard:workflow_run_knowledge';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_run_knowledge BEFORE UPDATE OF knowledge_ceiling,content_level ON public.workflow_runs
    FOR EACH ROW EXECUTE FUNCTION public.guard_workflow_run_knowledge();

CREATE FUNCTION public.guard_message_access_level() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.access_level IS DISTINCT FROM OLD.access_level THEN
        RAISE EXCEPTION 'message access level is immutable'
            USING ERRCODE='23514', CONSTRAINT='workflow_guard:message_access_level';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER message_access_level_immutable BEFORE UPDATE OF access_level ON public.messages
    FOR EACH ROW EXECUTE FUNCTION public.guard_message_access_level();
''';p.write_text(s)
