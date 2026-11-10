"""Fixed four-run actual-image release gate. Every run owns fresh disposable resources."""
import json
from types import SimpleNamespace


def run(args, runner):
    output = args.output.resolve()
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    summary = {"version": 1, "status": "FAIL", "runs": [],
        "scope": "actual private image, both storage modes and both required probe families"}
    identity = None
    try:
        for suite, mode in (("all", "disk"), ("all", "bucket"), ("e2e", "disk"), ("e2e", "bucket")):
            name = suite + "-" + mode
            summary["current_run"] = name
            case = {**vars(args), "output": output / name, "suite": suite, "private_deployment": mode}
            code = runner.run_private_deployment(SimpleNamespace(**case))
            if code != 0:
                raise RuntimeError("Required private acceptance run failed: " + name)
            report = json.loads((output / name / "report.json").read_text())
            cleanup = json.loads((output / name / "cleanup-receipt.json").read_text())
            if (set(cleanup) != {"version", "confirmed", "runtime_removed"}
                    or type(cleanup["version"]) is not int or cleanup["version"] != 1
                    or cleanup["confirmed"] is not True or cleanup["runtime_removed"] is not True):
                raise RuntimeError("Private acceptance cleanup is unconfirmed: " + name)
            if (output / name / "cleanup-pending.json").exists():
                raise RuntimeError("Private acceptance has pending resource recovery: " + name)
            if report.get("suite") != suite or report.get("storage_mode") != mode:
                raise RuntimeError("Private acceptance report has the wrong family or storage mode")
            observed = {"source": report["source_identity"]["fingerprint"], "image": report["image"],
                "openbot_helper_sha256": report["openbot_helper_sha256"]}
            if identity is None:
                identity = observed
            if observed != identity:
                raise RuntimeError("Private acceptance source, image or Openbot helper changed between runs")
            summary["runs"].append({"name": name, "status": "PASS", "checks": len(report["checks"]),
                "report_sha256": runner.digest(output / name / "report.json"),
                "cleanup_sha256": runner.digest(output / name / "cleanup-receipt.json")})
        summary.update(status="PASS", identity=identity)
        del summary["current_run"]
        return 0
    finally:
        with (output / "acceptance.json").open("x") as handle:
            handle.write(json.dumps(summary, indent=2) + "\n")
