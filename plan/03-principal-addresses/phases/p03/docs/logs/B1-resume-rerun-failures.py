import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

round_number = int(sys.argv[1]) if len(sys.argv) > 1 else 1

logs = Path(__file__).resolve().parent
binary = Path("/Users/mac02/Library/Caches/openbot/thr_jt43h4qc8d/target/debug/deps/open_bots-ecc73d9eac497eda")
inventory = subprocess.check_output([str(binary), "--list"], text=True)
(logs / f"B1-current-lib-tests{round_number + 7}.txt").write_text(inventory)
collected = {line[:-6] for line in inventory.splitlines() if line.endswith(": test")}
failures = json.loads((logs / "B1-resume-broad-failures1.json").read_text())
assert len(failures) == 76
selected = []
renamed = []
for item in failures:
    before = item["test"]
    after = before.replace("::onboarding_second_channel_error_rolls_back_every_effect", "::onboarding_second_principal_workflow_error_rolls_back_every_effect").replace("::a_hop_to_a_plain_channel_is_relayed", "::a_pure_workflow_hop_to_a_public_channel_is_relayed")
    assert after in collected, (before, after)
    selected.append(after)
    if before != after:
        renamed.append({"before": before, "replacement": after})
assert len(set(selected)) == 76
url = "postgresql://mac02@127.0.0.1:52471/pa03_codex_resume1_20261115"
env = os.environ | {"DATABASE_URL": url, "TEST_DATABASE_URL": url, "RUST_MIN_STACK": "16777216"}
output = logs / f"B1-original76-correction-tests{round_number}.txt"
with output.open("w") as stream:
    result = subprocess.run([str(binary), "--exact", *selected, "--nocapture"], env=env, stdout=stream, stderr=subprocess.STDOUT)
text = output.read_text()
assert "running 76 tests" in text
summary = re.findall(r"test result: .*", text)
record = {"selected_count": 76, "collected_count": len(collected), "renamed": renamed, "tests": selected, "exit": result.returncode, "summary": summary, "log": output.name, "log_sha256": hashlib.sha256(output.read_bytes()).hexdigest()}
(logs / f"B1-original76-correction-results{round_number}.json").write_text(json.dumps(record, indent=2) + "\n")
print(json.dumps({key: record[key] for key in ("selected_count", "collected_count", "exit", "summary", "log")}))
raise SystemExit(result.returncode)
