#!/usr/bin/env python3
"""Copy only this service. No parent Cargo configuration or assets enter the copy."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

root = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="as_extract_") as temporary:
    isolated = Path(temporary) / "attribution"
    shutil.copytree(root, isolated, symlinks=True,
                    ignore=shutil.ignore_patterns("target", ".git", ".env", ".contract-env", "__pycache__", "node_modules"))
    for path in isolated.rglob("*"):
        if path.is_symlink():
            raise RuntimeError("service extraction rejects symbolic links")
    mode = sys.argv[1] if len(sys.argv) > 1 else "full"
    # Browser tooling is installed independently, like the contract validator.
    # The copied source never imports a parent application or frontend asset.
    env = dict(os.environ)
    env.setdefault("ATTRIBUTION_PLAYWRIGHT_MODULE", str(root / "browser/node_modules/playwright/index.mjs"))
    subprocess.run([sys.executable, "scripts/verify.py", mode], cwd=isolated, check=True, env=env)
print("standalone service copy passed")
