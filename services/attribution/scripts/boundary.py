#!/usr/bin/env python3
"""Executable independence guard; extraction and separate DB roles provide runtime proof."""
import json
from pathlib import Path
import re
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]
IDENTIFIER = r'(?:"[a-z_][a-z_0-9]*"|[a-z_][a-z_0-9]*)'
SQL_REFERENCE = re.compile(r'\b(?:FROM|JOIN|UPDATE|INTO|TABLE)\s+(?:IF\s+(?:NOT\s+)?EXISTS\s+)?(?:LATERAL\s+)?(?!LATERAL\b)(' + IDENTIFIER + r'(?:\.' + IDENTIFIER + r')?)', re.I)
CATALOG = {"pg_roles", "pg_database", "pg_tables", "pg_namespace", "pg_class", "pg_auth_members", "_sqlx_migrations"}
OPENBOT_PACKAGES = {"openbot", "open_bots", "open-bots"}


def rust_strings(source):
    """Read complete ordinary/raw Rust string tokens, including embedded SQL quotes."""
    # The raw alternative must precede ordinary strings. Matching the same hash
    # delimiter preserves SQL identifier quotes instead of treating them as Rust.
    tokens = re.compile(r'(?:b|c)?r(?P<hashes>\#*)"(?P<raw>.*?)"(?P=hashes)|(?:b|c)?"(?P<normal>(?:\\.|[^"\\])*)"', re.S)
    for token in tokens.finditer(source):
        if token.group("raw") is not None:
            yield token.group("raw")
        else:
            value = token.group("normal")
            # SQL ownership only depends on identifiers/quotes; preserve other
            # escapes, which are interpreted by Rust/SQL rather than Python.
            yield value.replace('\\"', '"').replace('\\\n', '')


def inside(path, root):
    return path.resolve().is_relative_to(root.resolve())


def check(root, metadata):
    failures = []
    for package in metadata["packages"]:
        if package["name"].lower() in OPENBOT_PACKAGES:
            failures.append("resolved Openbot package")
        if package.get("source") is None:
            if not inside(Path(package["manifest_path"]), root):
                failures.append("resolved path dependency outside service: " + package["name"])
            elif Path(package["manifest_path"]).resolve() != (root / "Cargo.toml").resolve():
                failures.append("local wrapper packages are not supported by the single-package service: " + package["name"])
            for target in package.get("targets", []):
                if not inside(Path(target["src_path"]), root):
                    failures.append("Cargo target outside service: " + package["name"])
                elif not inside(Path(target["src_path"]), root / "src"):
                    failures.append("Cargo target outside audited src tree: " + package["name"])
        if str(package.get("source", "")).startswith("git+"):
            # The resolved hash alone is insufficient: require a manifest revision/tag too.
            if not re.search(r'[?&](?:rev|tag)=', package["source"]):
                failures.append("unpinned git dependency: " + package["name"])
    for manifest in root.rglob("Cargo.toml"):
        if "target" in manifest.relative_to(root).parts:
            continue
        data = tomllib.loads(manifest.read_text())
        for table in [data.get("dependencies", {}), data.get("dev-dependencies", {}), data.get("build-dependencies", {})]:
            for name, dep in table.items():
                if isinstance(dep, dict) and dep.get("workspace"):
                    # This package declares its own workspace; parent inheritance is forbidden.
                    failures.append("workspace dependency inheritance: " + name)
    migration_sql = "\n".join(p.read_text() for p in (root / "migrations").glob("*.sql"))
    owned = set(re.findall(r'CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?([a-z_][a-z_0-9]*)', migration_sql, re.I))
    for directory in (root / "src", root / "migrations", root / "assets", root / ".cargo"):
        for path in directory.rglob("*"):
            if path.is_symlink():
                failures.append("symbolic source link: " + str(path.relative_to(root)))
            if not path.is_file() or path.suffix not in (".rs", ".sql", ".js", ".css", ".toml", ".html"):
                continue
            source = path.read_text()
            if re.search(r'\b(?:openbot|open_bots)\s*::|\bextern\s+crate\s+(?:openbot|open_bots)\b|\b(?:OPENBOT|OPEN_BOTS)_[A-Z_]+', source):
                failures.append("private Openbot import/config: " + str(path.relative_to(root)))
            for reference in re.findall(r'(?:include(?:_str|_bytes)?!\s*\(|#\[path\s*=)\s*"([^"]+)"', source):
                if not inside(path.parent / reference, root):
                    failures.append("parent source/asset include: " + str(path.relative_to(root)))
            if path.suffix == ".rs" and re.search(r'(?:include(?:_str|_bytes)?!)\s*\(\s*(?!")\S', source):
                failures.append("computed source/asset include needs explicit local ownership: " + str(path.relative_to(root)))
            if path.suffix in (".js", ".css", ".html", ".toml") and re.search(r'\.\./', source):
                failures.append("parent asset/config reference: " + str(path.relative_to(root)))
            if path.suffix == ".sql":
                statements = [source]
            elif path.suffix == ".rs" and not path.name.endswith("_tests.rs") and path.name != "test_support.rs":
                # Restrict the SQL check to string contents; Rust `use` and prose are not SQL.
                statements = [value for value in rust_strings(source)
                              if re.match(r'\s*(?:SELECT|WITH|INSERT|UPDATE|DELETE|CREATE|ALTER|DROP)\b', value, re.I)]
            else:
                continue
            for sql in statements:
                sql = sql.replace('\\"', '"')
                # These UPDATE keywords introduce locking/upsert clauses, not tables.
                # Keep strings separate: a trailing FOR UPDATE must not consume the
                # first identifier in the next, unrelated query literal.
                sql = re.sub(r'\b(?:FOR\s+(?:NO\s+KEY\s+)?UPDATE|DO\s+UPDATE)\b', '', sql, flags=re.I)
                ctes = {name.lower() for name in re.findall(
                    r'(?:\bWITH|,)\s*([a-z_][a-z_0-9]*)\s*(?:\(\s*[a-z_][a-z_0-9]*(?:\s*,\s*[a-z_][a-z_0-9]*)*\s*\))?\s+AS\s*(?:(?:NOT\s+)?MATERIALIZED\s+)?\(', sql, re.I)}
                for table in SQL_REFERENCE.findall(sql):
                    table = table.lower().replace('"', '')
                    if table.startswith("public.") or table.startswith("pg_catalog."):
                        table = table.split(".", 1)[1]
                    if table not in owned | CATALOG | ctes:
                        failures.append(f"SQL table outside owned migrations/catalog: {table} in {path.relative_to(root)}")
                if re.search(r'\b(?:dblink|postgres_fdw)\b', sql, re.I):
                    failures.append("cross-database SQL access: " + str(path.relative_to(root)))
    return failures


def main():
    result = subprocess.run(["cargo", "metadata", "--locked", "--offline", "--format-version", "1"],
                            cwd=ROOT, check=True, text=True, capture_output=True)
    failures = check(ROOT, json.loads(result.stdout))
    if failures:
        raise SystemExit("\n".join(failures))
    print("attribution boundary passed: resolved dependencies, source/assets/config, owned SQL")


if __name__ == "__main__":
    main()
