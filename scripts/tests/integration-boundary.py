#!/usr/bin/env python3
"""Mutation tests exercise the real integration boundary checker in tiny crates."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('boundary', Path(__file__).parents[1]/'integration-boundary.py')
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)


class Boundary(unittest.TestCase):
    def check(self, source, extra=None, files=None):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for path, value in {'src/lib.rs':'mod application;\n'+source,
                                'src/application.rs':'pub mod integrations;',
                                'src/application/integrations/mod.rs': extra or 'pub enum Reply { Done {} }',
                                **(files or {})}.items():
                file = root/path; file.parent.mkdir(parents=True, exist_ok=True); file.write_text(value)
            return gate.violations(root)

    def test_clean(self): self.assertEqual(self.check(''), [])
    def test_sql(self): self.assertTrue(self.check('const Q:&str="SELECT envelope FROM integration_accounts";'))
    def test_raw_sql(self): self.assertTrue(self.check('const Q:&str=r##"SELECT * FROM integration_use_grants"##;'))
    def test_teardown_custody(self): self.assertTrue(self.check('const Q:&str="SELECT * FROM integration_teardown_intents";'))
    def test_test_filename_is_not_a_bypass(self):
        self.assertTrue(self.check('mod misleading_tests { const Q:&str="integration_accounts"; }'))
    def test_test_only_items_are_excluded(self):
        self.assertEqual(self.check('#[cfg(test)] mod tests { const Q:&str="integration_accounts"; }'), [])
    def test_nested_comments(self):
        self.assertEqual(self.check('/* /* nested */ integration_accounts */'), [])
    def test_host_type(self): self.assertTrue(self.check('', 'use crate::application::workflow::WorkflowFence;'))
    def test_grouped_host_import_with_alias(self):
        self.assertTrue(self.check('', 'use crate::application::{workflow::{WorkflowActor as Actor}}; pub struct Value { actor: Actor }'))
    def test_private_import(self): self.assertTrue(self.check('use crate::application::integrations::private::CredentialRecord;'))
    def test_grouped_private_port_import(self):
        self.assertTrue(self.check('use crate::application::{integrations::{private::{CredentialStore as Store}}};'))
    def test_credential_derive(self): self.assertTrue(self.check('', '#[derive(Serialize)] pub struct CredentialRecord { value: SecretString }'))
    def test_manual_serialization(self): self.assertTrue(self.check('', 'impl serde::Serialize for VerifiedOwner {}'))
    def test_alias_reply(self):
        self.assertTrue(self.check('', 'type Leaked = CredentialRecord; pub enum Reply { Leak { value: Leaked } }'))
    def test_command_secret_cannot_be_a_reply(self):
        self.assertTrue(self.check('', '#[derive(Serialize)] pub struct SecretSubmission(SecretString); pub enum Reply { Leak(SecretSubmission) }'))
    def test_compiling_imported_alias_mutation(self):
        source = (Path(__file__).parent/'fixtures/integration-boundary-alias.rs').read_text()
        failures = self.check('', source)
        self.assertIn('Leaked: serializable runtime proof or credential-bearing type', failures)
        self.assertIn('Reply: credential/runtime proof reachable from gateway result', failures)
    def test_reexported_alias_chain(self):
        self.assertTrue(self.check('', 'pub use secrecy::SecretString as Hidden; pub use self::Hidden as Payload; pub struct Leaked(Payload); impl serde::Serialize for Leaked {} pub enum Reply { Leak(Leaked) }'))
    def test_serialization_trait_alias(self):
        self.assertTrue(self.check('', 'use serde::Serialize as Encode; use secrecy::SecretString as Hidden; pub struct Leaked(Hidden); impl Encode for Leaked {}'))
    def test_derive_trait_alias(self):
        self.assertTrue(self.check('', 'use serde::Serialize as Encode; #[derive(Encode)] pub struct Leaked(SecretString);'))
    def test_valid_aliased_metadata(self):
        self.assertEqual(self.check('', 'use std::string::String as Label; use serde::Serialize as Encode; #[derive(Encode)] pub struct Metadata { label: Label } pub enum Reply { Value(Metadata) }'), [])
    def test_alias_reexport_through_external_helper(self):
        self.assertTrue(self.check('mod aliases;', 'use crate::aliases::Hidden as Payload; pub struct Leaked(Payload); impl serde::Serialize for Leaked {} pub enum Reply { Leak(Leaked) }',
                                   {'src/aliases.rs':'pub use secrecy::SecretString as Hidden;'}))
    def test_test_only_struct_field_does_not_consume_following_item(self):
        self.assertTrue(self.check('', 'pub struct Metadata { #[cfg(test)] hidden: bool, label: String } pub struct Leaked(SecretString); impl serde::Serialize for Leaked {} pub enum Reply { Leak(Leaked) }'))


if __name__ == '__main__': unittest.main()
