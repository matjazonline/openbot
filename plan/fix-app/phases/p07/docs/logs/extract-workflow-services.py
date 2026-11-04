from pathlib import Path
import re
p=Path('src/infra/workflow.rs');s=p.read_text();a=s.index('    fn authorizer(&self)');spawn=s.index('    pub fn spawn(',a);b=s.rfind('\n\n',a,spawn)+2
methods=s[a:b].rstrip();methods=methods.replace('    fn schedule_materializer(', '    pub(super) fn schedule_materializer(')
d=Path('src/infra/workflow');d.mkdir(exist_ok=True)
(d/'services.rs').write_text('''//! HTTP and application command services; this view owns no handler or worker loop.
use super::*;
pub struct WorkflowServices {
    pub(super) persistence: PostgresPersistence,
    pub(super) notices: DecisionNotices,
    pub(super) templates: ProvisioningDeployment,
    pub(super) monitoring: Arc<dyn MonitoringService>,
    pub(super) worker_wakeups: Arc<Notify>,
}
impl WorkflowServices {
'''+methods+'\n}\n')
s=s[:a]+'''    pub fn services(&self) -> Arc<WorkflowServices> { self.services.clone() }

'''+s[b:]
s=s.replace('pub struct WorkflowEngine {\n    persistence: PostgresPersistence,','pub struct WorkflowEngine {\n    services: Arc<WorkflowServices>,',1).replace('    notices: DecisionNotices,\n    /// The shipped templates assistant provisioning compiles from (p08 D2.5).\n    templates: ProvisioningDeployment,\n','',1)
s=s.replace('''        Ok(Self {
            persistence,
            handler,
            notices,
            templates,''','''        let worker_wakeups = Arc::new(Notify::new());
        let services = Arc::new(WorkflowServices {
            persistence, notices, templates, monitoring: monitoring.clone(), worker_wakeups: worker_wakeups.clone(),
        });
        Ok(Self {
            services,
            handler,''',1).replace('            worker_wakeups: Arc::new(Notify::new()),','            worker_wakeups,',1)
s=s.replace('self.persistence','self.services.persistence').replace('self.notices','self.services.notices').replace('self.templates','self.services.templates').replace('self.service()','self.services.service()').replace('self.schedule_materializer()','self.services.schedule_materializer()')
i=s.index('/// Registered step kinds');s=s[:i]+'mod services;\npub use services::WorkflowServices;\n\n'+s[i:];p.write_text(s)

logs=Path('plan/fix-app/phases/p07/docs/logs')
files=set(re.findall(r'src/[^:\s()]+\.rs',(logs/'workflow-engine-current-sites.txt').read_text()))
files.update(re.findall(r'src/[^:\s()]+\.rs',(logs/'workflow-service-current-sites.txt').read_text()))
service_names=['service','authorization','schedule_run_now','reads','authoring_reads','run_views','selectable_bundle','step_catalogue','sample','decisions','jev_connections','workflow_tools','template_catalogue','template_copies','reconciliation','wake_workers','onboarding_persistence','assistant_channels','bindings','definitions']

def balanced_end(s,start):
 depth=0;quoted=False;escaped=False
 for j in range(start,len(s)):
  c=s[j]
  if quoted:
   if escaped:escaped=False
   elif c=='\\':escaped=True
   elif c=='"':quoted=False
  elif c=='"':quoted=True
  elif c=='(':depth+=1
  elif c==')':
   depth-=1
   if depth==0:return j+1
 raise AssertionError('unbalanced call')

for name in sorted(files):
 p=Path(name)
 if not p.exists():continue
 s=p.read_text()
 if name.startswith('src/adapters/http/'):
  # Constructor fixtures keep runtime construction explicit, immediately retaining only its view.
  edits=[]
  for m in re.finditer(r'Arc::new\(\s*WorkflowEngine::for_tests\(',s):
   a=m.start();end=balanced_end(s,s.index('(',a));inner=s[s.index('(',a)+1:end-1].strip()
   edits.append((a,end,inner+'.services()'))
  for a,b,new in reversed(edits):s=s[:a]+new+s[b:]
  s=re.sub(r'\bWorkflowEngine\b','WorkflowServices',s)
  s=s.replace('WorkflowServices::for_tests(', 'crate::infra::workflow::WorkflowEngine::for_tests(')
 else:
  methods='|'.join(service_names)
  s=re.sub(r'\b(engine|workflow)\.('+methods+r')\(',r'\1.services().\2(',s)
 p.write_text(s)
print('Worker engine and HTTP service capabilities separated')
