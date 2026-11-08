from pathlib import Path
import subprocess,sys
runner=Path(__file__).with_name('run-check.py')
filters=['agent_history_','workflow_harness_','workflow_agent_','autonomous_agent_authority_','knowledge_search_','run_retention_native_checkpoint','workflow_control_','workflow_recovery_','model_steps::','channel_reply_e2e_','autonomous_agent_samples_','--skip','dump_autonomous_workflow_agent_picker_browser_fixtures']
checks=[
 ('findings-style-02',['cargo','fmt','--all']),
 ('findings-affected-list-04',['cargo','test','--locked','--offline','--lib','--','--list',*filters]),
 ('findings-affected-runtime-04',['scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','--',*filters]),
 ('sqlx-prepare-findings-02',['cargo','sqlx','prepare','--','--locked','--offline','--all-targets']),
 ('sqlx-check-findings-02',['cargo','sqlx','prepare','--check','--','--locked','--offline','--all-targets']),
 ('offline-all-targets-findings-02',['cargo','check','--locked','--offline','--all-targets']),
 ('clippy-findings-02',['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings']),
 ('fmt-findings-02',['cargo','fmt','--all','--','--check']),
 ('diff-findings-02',['git','diff','--check']),
 ('graft-findings-02',['graft','build']),
]
for name,command in checks:
 result=subprocess.run([sys.executable,str(runner),name,*command])
 if result.returncode:sys.exit(result.returncode)
