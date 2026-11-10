#!/usr/bin/env python3
"""Service independence checks (implementation-decisions D1). Usage:
  boundary_check.py [--service DIR]        static checks
  boundary_check.py --probe                also run the poisoned-parent build probe
"""
import json, os, pathlib, re, shutil, subprocess, sys, tempfile, tomllib

ARRAY_FLAGS = re.compile(r"^(build\.(rustflags|rustdocflags)|target\.[^.]+\.(rustflags|rustdocflags))$")


def leaves(table, prefix=""):
    for key, value in table.items():
        dotted = f"{prefix}{key}"
        if isinstance(value, dict):
            yield from leaves(value, dotted + ".")
        else:
            yield dotted, value


def parent_config_problems(service):
    own = dict(leaves(tomllib.loads((service / ".cargo/config.toml").read_text())))
    problems = []
    for directory in service.resolve().parents:
        for name in ("config.toml", "config"):
            path = directory / ".cargo" / name
            if not path.is_file():
                continue
            for key, value in leaves(tomllib.loads(path.read_text())):
                if ARRAY_FLAGS.match(key) or isinstance(value, list):
                    # Cargo concatenates arrays across files; the service cannot cancel them.
                    problems.append(f"{path}: {key} is set by a parent and always leaks into the build")
                elif key not in own:
                    problems.append(f"{path}: {key} is not overridden by the service's .cargo/config.toml")
    return problems


def metadata_problems(service):
    out = subprocess.run(["cargo", "metadata", "--format-version", "1", "--locked", "--offline"],
                         cwd=service, capture_output=True, text=True)
    if out.returncode != 0:
        return [f"cargo metadata failed: {out.stderr.strip()[:400]}"]
    data = json.loads(out.stdout)
    root = str((service / "Cargo.toml").resolve())
    problems = []
    for package in data["packages"]:
        source = package.get("source")
        if package["manifest_path"] != root and source is None:
            problems.append(f"path dependency {package['name']} at {package['manifest_path']}")
        if source and source.startswith("git+"):
            problems.append(f"git dependency {package['name']} ({source})")
        if package["name"] == "open_bots":
            problems.append("depends on open_bots")
    return problems


def source_problems(service):
    problems = []
    root = service.resolve()
    include = re.compile(r'(?:include_str|include_bytes)!\("([^"]+)"\)|#\[path = "([^"]+)"\]')
    patterns = [(re.compile(r"\bopen_bots\b"), "names the OpenBot crate"),
                (re.compile(r"(env::var|getenv)\(\"DATABASE_URL"), "reads DATABASE_URL"),
                (re.compile(r"(?<!--no-)dotenv"), "loads .env files")]
    files = list((service / "src").rglob("*.rs")) + [p for p in (service / "scripts").iterdir() if p.is_file()]
    files += [service / "Dockerfile", service / "Cargo.toml"]
    for path in files:
        if not path.is_file() or path.name in ("boundary_check.py", "boundary_tests.rs"):
            continue
        for number, line in enumerate(path.read_text().splitlines(), 1):
            if line.strip().startswith(("//", "#!")):
                continue
            where = f"{path.relative_to(service)}:{number}"
            if path.suffix == ".rs":
                for match in include.finditer(line):
                    target = (path.parent / (match.group(1) or match.group(2))).resolve()
                    if root not in target.parents and target != root:
                        problems.append(f"{where} includes a file outside the service")
            elif "../.." in line:
                problems.append(f"{where} reaches above the service directory")
            for pattern, why in patterns:
                if pattern.search(line):
                    problems.append(f"{where} {why}")
    return problems


# D1 layering: the pure domain and the use cases name no I/O crate and no adapter.
LAYER_CRATES = re.compile(r"(?<![\w:])(?:sqlx|axum|reqwest|rmcp|fastembed|ort)::|\bcrate::adapters\b|\bextern\s+crate\s+(?:sqlx|axum|reqwest|rmcp|fastembed|ort)\b")


def layering_problems(service):
    problems = []
    for layer in ("domain", "application"):
        for path in sorted((service / "src" / layer).rglob("*.rs")):
            for number, line in enumerate(path.read_text().splitlines(), 1):
                if line.strip().startswith("//"):
                    continue
                if LAYER_CRATES.search(line):
                    problems.append(f"{path.relative_to(service)}:{number} names an I/O crate or adapter from src/{layer}/")
    return problems


def check(service):
    return (parent_config_problems(service) + metadata_problems(service) + source_problems(service)
            + layering_problems(service))


def probe(service):
    """Copy the service under a poisoned parent: the check must refuse the parent rustflags, and
    with them removed the probe test must see only the service's own environment."""
    with tempfile.TemporaryDirectory(prefix="memory-probe-") as tmp:
        parent = pathlib.Path(tmp)
        (parent / ".cargo").mkdir()
        (parent / ".env").write_text("DATABASE_URL=postgres://memory-poison@127.0.0.1:1/x\nSQLX_OFFLINE=memory-poison\n")
        config = parent / ".cargo" / "config.toml"
        config.write_text('[env]\nRUST_MIN_STACK = "1"\n[build]\nrustflags = ["--cfg", "memory_poison"]\n')
        copy = parent / "memory"
        shutil.copytree(service, copy, ignore=shutil.ignore_patterns("target", ".tools", "model"))
        found = parent_config_problems(copy)
        if not any("build.rustflags" in problem for problem in found):
            return ["probe: a parent build.rustflags was not detected"]
        poison = copy / "src" / "domain" / "probe_poison.rs"
        poison.write_text("use sqlx::Row;\nfn f() -> crate::adapters::http::HttpSettings { todo!() }\n")
        found = layering_problems(copy)
        poison.unlink()
        if len(found) != 2:
            return [f"probe: the layering rule missed an I/O crate or adapter in src/domain/: {found}"]
        config.write_text('[env]\nRUST_MIN_STACK = "1"\n')
        if parent_config_problems(copy):
            return ["probe: a parent key the service overrides was reported"]
        # A dedicated target directory (the poisoned parent changes the build environment);
        # under an explicit CARGO_TARGET_DIR it lives beside that one, so repeated runs reuse it.
        target = pathlib.Path(os.environ.get("CARGO_TARGET_DIR") or service / "target")
        env = dict(os.environ, CARGO_TARGET_DIR=str(target / "probe"))
        env.pop("RUST_MIN_STACK", None)
        run = subprocess.run(["cargo", "test", "--locked", "--offline", "--lib", "memory_boundary_environment_probe"],
                             cwd=copy, env=env, capture_output=True, text=True)
        if run.returncode != 0 or "1 passed" not in run.stdout:
            return ["probe: memory_boundary_environment_probe failed under a poisoned parent:\n" + run.stdout[-2000:] + run.stderr[-2000:]]
    return []


def main():
    service = pathlib.Path(__file__).resolve().parent.parent
    problems = check(service)
    if "--probe" in sys.argv[1:] and not problems:
        problems += probe(service)
    for problem in problems:
        print(f"boundary: {problem}", file=sys.stderr)
    if problems:
        return 1
    print("boundary: ok" + (" (with poisoned-parent probe)" if "--probe" in sys.argv[1:] else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main())
