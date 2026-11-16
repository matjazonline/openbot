#!/usr/bin/env python3
import json
from pathlib import Path
import tempfile
import unittest

from boundary import check


class BoundaryTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="as_boundary_")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name) / "service"
        for directory in ("src", "migrations", "assets", ".cargo"):
            (self.root / directory).mkdir(parents=True)
        (self.root / "Cargo.toml").write_text('[package]\nname="fixture"\nversion="0.1.0"\n')
        (self.root / "migrations/001.sql").write_text("CREATE TABLE merchants (id bigint);")
        self.metadata = {"packages": [{"name": "fixture", "source": None, "manifest_path": str(self.root / "Cargo.toml")}]}

    def source(self, value):
        (self.root / "src/lib.rs").write_text(value)
        return check(self.root, self.metadata)

    def test_attribution_boundary_local_source_and_owned_sql_pass(self):
        self.assertEqual(self.source('include_str!("../assets/app.css"); query("SELECT id FROM merchants");'), [])

    def test_attribution_boundary_transitive_wrapper_cannot_hide_parent_dependency(self):
        self.metadata["packages"].append({"name": "innocent_wrapper", "source": None, "manifest_path": str(self.root.parent / "wrapper/Cargo.toml")})
        self.assertTrue(any("path dependency" in v for v in check(self.root, self.metadata)))

    def test_attribution_boundary_private_import_and_parent_asset_are_rejected(self):
        for source in ('use openbot::Config;', 'use open_bots::Config;', 'extern crate open_bots as innocent;', 'include_str!("../../assets/app.css");', 'env!("OPENBOT_SECRET");', 'include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../src/lib.rs"));'):
            with self.subTest(source=source):
                self.assertTrue(self.source(source))

    def test_attribution_boundary_foreign_sql_is_rejected(self):
        for sql in ("SELECT id FROM companies", 'SELECT id FROM "companies"', 'SELECT id FROM "public"."companies"', "INSERT INTO public.users VALUES (1)", "DELETE FROM sessions", "SELECT * FROM secret.companies", "SELECT * FROM dblink('x')"):
            with self.subTest(sql=sql):
                self.assertTrue(self.source("query(" + json.dumps(sql) + ");"))

    def test_attribution_boundary_unpinned_git_is_rejected(self):
        self.metadata["packages"].append({"name": "external", "source": "git+https://example.test/lib#abcd", "manifest_path": "/cache/Cargo.toml"})
        self.assertTrue(any("unpinned git" in v for v in check(self.root, self.metadata)))

    def test_attribution_boundary_actual_openbot_package_is_rejected_even_with_pinned_source(self):
        self.metadata["packages"].append({"name": "open_bots", "source": "git+https://example.test/openbot?rev=abc#abc", "manifest_path": "/cache/Cargo.toml"})
        self.assertTrue(any("resolved Openbot" in v for v in check(self.root, self.metadata)))

    def test_attribution_boundary_in_service_wrappers_cannot_escape_the_scanned_layout(self):
        wrapper = self.root / "wrapper"
        (wrapper / "src").mkdir(parents=True)
        (wrapper / "src/lib.rs").write_text('include_str!("../../../private-openbot.env");')
        self.metadata["packages"].append({"name": "wrapper", "source": None, "manifest_path": str(wrapper / "Cargo.toml"), "targets": [{"src_path": str(wrapper / "src/lib.rs")}]})
        self.assertTrue(any("local wrapper packages" in v for v in check(self.root, self.metadata)))

    def test_attribution_boundary_custom_targets_outside_scanned_source_are_rejected(self):
        self.metadata["packages"][0]["targets"] = [{"src_path": str(self.root / "build.rs")}]
        self.assertTrue(any("outside audited src" in v for v in check(self.root, self.metadata)))

    def test_attribution_boundary_raw_sql_preserves_quoted_and_qualified_identifiers(self):
        for hashes in ("#", "##", "###"):
            for table in ('"companies"', '"public"."companies"', 'secret."companies"'):
                with self.subTest(hashes=hashes, table=table):
                    self.assertTrue(self.source(f'sqlx::query(r{hashes}"SELECT id FROM {table}"{hashes});'))
            self.assertEqual(self.source(f'sqlx::query(r{hashes}"SELECT id FROM "public"."merchants""{hashes});'), [])

    def test_attribution_boundary_materialized_ctes_and_lateral_keep_nested_checks(self):
        for table in ("merchants", "companies"):
            for materialized in ("", "MATERIALIZED ", "NOT MATERIALIZED "):
                sql = f"WITH facts(id,n) AS {materialized}(SELECT id,1 FROM {table}), data AS (SELECT * FROM facts) SELECT * FROM data JOIN LATERAL (SELECT id FROM {table}) v ON true"
                result = self.source("query(" + json.dumps(sql) + ");")
                self.assertEqual(bool(result), table == "companies")
        self.assertTrue(self.source('query("WITH facts AS (SELECT id FROM merchants) SELECT * FROM facts JOIN LATERAL (SELECT id FROM companies) x ON true");'))
        self.assertTrue(self.source('query("WITH facts AS (SELECT id FROM merchants) SELECT * FROM facts"); query("SELECT * FROM facts");'))

    def test_attribution_boundary_locking_and_separate_ctes_keep_table_checks(self):
        for clause in ("FOR UPDATE", "FOR UPDATE SKIP LOCKED", "FOR NO KEY UPDATE"):
            self.assertEqual(self.source(f'query("SELECT id FROM merchants {clause}"); query("SELECT id FROM merchants");'), [])
            self.assertTrue(self.source(f'query("SELECT id FROM companies {clause}");'))
        self.assertEqual(self.source('query("INSERT INTO merchants (id) VALUES (1) ON CONFLICT (id) DO UPDATE SET id = 2");'), [])
        self.assertTrue(self.source('query("WITH companies AS (SELECT id FROM merchants) SELECT id FROM companies"); query("SELECT id FROM companies");'))


if __name__ == "__main__":
    unittest.main(verbosity=2)
