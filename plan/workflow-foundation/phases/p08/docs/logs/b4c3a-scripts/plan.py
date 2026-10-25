import re, sys
sys.path.insert(0, '.')
from migseg import load
T = set(open('tables.txt').read().split())
F = set(open('funcs.txt').read().split())
# Explicitly named segments on surviving tables (from the verified inventory).
NAMED = {
  # background_tasks
  'background_tasks background_tasks_company_source_message_channel_key',
  'background_tasks background_tasks_awaited_outreach_fk', 'background_tasks background_tasks_owner_principal_fk',
  'background_tasks background_tasks_source_message_fk', 'background_tasks background_tasks_transition_approval_fk',
  'background_tasks background_tasks_transition_outreach_fk',
  'background_tasks_pending_ready_idx', 'background_tasks_thread_correlation_match_idx',
  'background_tasks_unsettled_owner_idx', 'background_tasks_waiting_due_idx',
  # task_harness_runs
  'task_harness_runs_scope',
  # message_deliveries
  'message_deliveries message_deliveries_task_fk', 'message_deliveries_task_idx',
}
def table_of(seg):
    if seg['type'] in ('CONSTRAINT','FK CONSTRAINT','TRIGGER','DEFAULT'):
        return seg['name'].split(' ')[0]
    if seg['type'] == 'INDEX':
        m = re.search(r' ON (?:ONLY )?public\.(\w+)', seg['body']); return m.group(1) if m else None
    if seg['type'] == 'COMMENT':
        m = re.match(r'(?:TABLE|COLUMN) (\w+)', seg['name']); return m.group(1) if m else None
    if seg['type'] == 'TABLE': return seg['name']
    return None
def plan():
    lines, segs, tail = load()
    drop = []
    for s in segs:
        why = None
        t = table_of(s)
        if t in T: why = 'table ' + t
        elif s['type'] == 'FUNCTION' and s['name'].split('(')[0] in F: why = 'function'
        elif s['type'] == 'FK CONSTRAINT' and any(re.search(r'REFERENCES public\.%s\(' % x, s['body']) for x in T): why = 'fk to dropped'
        elif s['type'] == 'TRIGGER' and any(re.search(r'EXECUTE FUNCTION public\.%s\(' % f, s['body']) for f in F): why = 'trigger of dropped fn'
        elif s['name'] in NAMED: why = 'named'
        if why: drop.append((s, why))
    return lines, segs, tail, drop
if __name__ == '__main__':
    lines, segs, tail, drop = plan()
    dropped = {id(s) for s, _ in drop}
    from collections import Counter
    print('drop', len(drop), Counter(w.split(' ')[0] for _, w in drop))
    for s, w in drop:
        if w in ('fk to dropped', 'trigger of dropped fn', 'named'):
            print('  ', w, '|', s['type'], '|', s['name'])
    print('NAMED not found:', [n for n in NAMED if not any(s['name']==n for s in segs)])
    pat = re.compile(r'\b(' + '|'.join(sorted(T | F, key=len, reverse=True)) + r')\b')
    print('--- surviving segments mentioning dropped names')
    for s in segs:
        if id(s) in dropped: continue
        hits = sorted(set(pat.findall(s['body'])))
        if hits: print(s['start']+1, s['type'], s['name'], hits)
    print('--- tail mentions')
    for i in range(tail, len(lines)):
        hits = sorted(set(pat.findall(lines[i])))
        if hits: print(i+1, hits, lines[i][:100])
