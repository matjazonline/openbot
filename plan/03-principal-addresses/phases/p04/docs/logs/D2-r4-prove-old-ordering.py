"""Run the real race test without the publication lock, then always restore source."""
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess

root = Path.cwd()
source = root / "src/adapters/mcp/mod.rs"
logs = root / "plan/03-principal-addresses/phases/p04/docs/logs"
original = source.read_bytes()
guard = b"            let _registration = self.cleanup.gate.lock().await;\n"
assert original.count(guard) == 1
mutated = original.replace(guard, b"            // Counterfactual: publication has no cleanup lock.\n", 1)
(logs / "D2-r4-corrected-mod.source.txt").write_bytes(original)
command = ["scripts/test-network-isolation.sh", "cargo", "test", "--locked", "--offline", "--lib",
           "mcp_enqueue_preemption_cancels_original_call_before_shutdown_deletes_session", "--", "--nocapture"]


def terminate(_number, _frame):
    raise SystemExit("Interrupted; restoring corrected source")


signal.signal(signal.SIGTERM, terminate)
process = None
try:
    source.write_bytes(mutated)
    with (logs / "D2-r4-old-ordering1.txt").open("w") as output:
        process = subprocess.Popen(command, stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
        status = process.wait(timeout=900)
    output = (logs / "D2-r4-old-ordering1.txt").read_text()
    assert status == 101 and "test result: FAILED" in output, "old ordering must fail the real test"
    assert "exactly the dispatched request must be cancelled" in output, "failure must be the actual wire order"
    assert "test must release the SDK handoff" not in output, "probe timeout is not regression proof"
    result = {"command": command, "expected_exit": 101, "actual_exit": status,
              "proof": "old ordering fails exact real cancellation-before-DELETE assertion",
              "corrected_source_sha256": hashlib.sha256(original).hexdigest(),
              "mutation_sha256": hashlib.sha256(mutated).hexdigest()}
    (logs / "D2-r4-old-ordering-proof.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result), flush=True)
finally:
    if process is not None and process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()
    source.write_bytes(original)
    assert source.read_bytes() == original
    print("Corrected source restored", flush=True)
