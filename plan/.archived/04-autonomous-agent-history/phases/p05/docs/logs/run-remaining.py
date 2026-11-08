from pathlib import Path
import subprocess,sys
l=Path(__file__).resolve().parent
steps=[
 [sys.executable,str(l/'run-check.py'),'knowledge-cc-final01','scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','--','knowledge_e2e_cc_and_member_demotion_mid_reply_refuse_delivery_and_show_typed_attention'],
 [sys.executable,str(l/'run-final-build.py')],
 [sys.executable,str(l/'run-focused.py'),'final-focused-list-02']]
for command in steps:
 result=subprocess.run(command)
 if result.returncode:sys.exit(result.returncode)
