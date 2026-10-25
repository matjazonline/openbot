#!/usr/bin/env python3
"""Exercise the gate with effect dependencies in current and newly added module paths."""
import importlib.util
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest
import sys
sys.dont_write_bytecode=True

spec=importlib.util.spec_from_file_location('boundary',Path(__file__).resolve().parents[1]/'workflow-sample-boundary.py')
boundary=importlib.util.module_from_spec(spec);spec.loader.exec_module(boundary)

class BoundaryTests(unittest.TestCase):
    def check(self,source,allowed):
        with TemporaryDirectory() as temporary:
            root=Path(temporary)
            for name in ['mod.rs','children.rs','tools.rs','new/effects.rs']:
                (root/'mod.rs').write_text('')
                path=root/name;path.parent.mkdir(exist_ok=True);path.write_text(source)
                self.assertEqual(not boundary.violations(root),allowed,name)
                path.unlink()
    def test_effect_capabilities_fail(self):
        for source in [
            'use crate::adapters::persistence::PostgresPersistence;',
            'use crate::application::workflow::{actions::ActionIntents};',
            'use crate::infra::workflow::WorkflowEngine;',
            'fn run(){reqwest::get("https://example.test");}',
            'fn run(){std::process::Command::new("curl");}',
            'fn run(){std::fs::read("credentials");}',
            'use tokio::{net::TcpStream};',
            'fn run(){::std::fs::read("credentials");}',
            'fn run(){::std::process::Command::new("curl");}',
            'fn run(){std :: fs :: read("credentials");}',
            'fn run(){::std /* comment */ :: process :: Command :: new("curl");}',
            'fn run(){::reqwest::get("https://example.test");}',
            'use ::std::{fs};',

            'mod tools_tests;', 'include!("effect.rs");',
        ]: self.check(source,False)
    def test_missing_root_fails_closed(self):
        with TemporaryDirectory() as temporary:self.assertTrue(boundary.violations(Path(temporary)))
    def test_pure_contracts_and_explicit_test_handling(self):
        self.check('use crate::application::workflow::{batch::{execute_pure, PureCompletion}}; use std::sync::Arc;',True)
        with TemporaryDirectory() as temporary:
            root=Path(temporary)
            (root/'tests.rs').write_text('use crate::adapters::persistence::PostgresPersistence;')
            (root/'mod.rs').write_text('#[cfg(test)]\nmod tests;')
            self.assertEqual(boundary.violations(root),[])
            (root/'mod.rs').write_text('mod tests;')
            self.assertTrue(boundary.violations(root))

if __name__=='__main__':unittest.main()
