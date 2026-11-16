"""Wire-contract/fixture checks; live authorization and ledger proof belong to Rust tests."""

import copy
import json
from pathlib import Path
import sys
import unittest

from jsonschema import Draft202012Validator, FormatChecker
from openapi_spec_validator import validate

ROOT = Path(__file__).resolve().parent


def load(name):
    return json.loads((ROOT / name).read_text())


API = load("attribution.openapi.json")
IDENTITY = load("openbot-identity.openapi.json")


def validator(document, schema):
    return Draft202012Validator(
        {"$ref": f"#/components/schemas/{schema}", "components": document["components"]},
        format_checker=FormatChecker(),
    )


def operations(document):
    for path, methods in document["paths"].items():
        for method, operation in methods.items():
            yield path, method, operation


class AttributionContracts(unittest.TestCase):
    def test_attribution_contract_both_openapi_documents_validate(self):
        for document in (API, IDENTITY):
            with self.subTest(api=document["info"]["title"]):
                validate(document)
                ids = [op["operationId"] for _, _, op in operations(document)]
                self.assertEqual(len(ids), len(set(ids)))

    def test_attribution_contract_examples_match_wire_schemas(self):
        for filename, schema in [
            ("paid", "Conversion"),
            ("refund", "Conversion"),
            ("checkout", "CheckoutWrite"),
            ("observation", "Observation"),
            ("lifecycle", "CompanyStatus"),
            ("analytics", "Analytics"),
            ("credential-replay", "CredentialReplay"),
        ]:
            with self.subTest(example=filename):
                validator(API, schema).validate(load(f"examples/{filename}.json"))
        validator(IDENTITY, "TokenRequest").validate(load("examples/token-request.json"))

    def test_attribution_contract_merchant_payloads_cannot_select_actor_or_tenant(self):
        for field in ("company_id", "store_id", "user_id"):
            for filename, schema in [("paid", "Conversion"), ("checkout", "CheckoutWrite")]:
                with self.subTest(field=field, schema=schema):
                    request = load(f"examples/{filename}.json")
                    request[field] = "11111111-1111-4111-8111-111111111111"
                    self.assertFalse(validator(API, schema).is_valid(request))
        request = load("examples/token-request.json")
        request["user_id"] = "11111111-1111-4111-8111-111111111111"
        self.assertFalse(validator(IDENTITY, "TokenRequest").is_valid(request))

    def test_attribution_contract_public_observations_cannot_describe_sales(self):
        event = load("examples/observation.json")
        for change in ({"kind": "paid"}, {"consent": False}, {"amount": 1200}):
            self.assertFalse(validator(API, "Observation").is_valid({**event, **change}))
        self.assertFalse(
            validator(API, "Observation").is_valid({**event, "click_id": None, "link_id": None})
        )
        for path in ("/v1/conversions", "/v1/checkouts"):
            self.assertEqual(API["paths"][path]["post"]["security"], [{"StoreKey": []}])
        self.assertEqual(
            API["paths"]["/internal/v1/company-lifecycle"]["post"]["security"],
            [{"Lifecycle": []}],
        )

    def test_attribution_contract_money_currency_and_collection_bounds(self):
        paid = load("examples/paid.json")
        for change in (
            {"currency": "XXX"},
            {"merchandise_amount": 1.25},
            {"merchandise_amount": -1},
            {"merchandise_amount": 9000000000001},
            {"lines": []},
            {"lines": [paid["lines"][0]] * 101},
        ):
            self.assertFalse(validator(API, "Conversion").is_valid({**paid, **change}))
        checkout = load("examples/checkout.json")
        checkout["candidate_click_ids"] *= 17
        self.assertFalse(validator(API, "CheckoutWrite").is_valid(checkout))
        # Cross-field sums, lexical integer parsing, and scoped IDs require runtime checks.

    def test_attribution_contract_identity_pkce_and_credential_classes(self):
        request = load("examples/token-request.json")
        for change in ({"code_verifier": "short"}, {"grant_type": "password"}):
            self.assertFalse(validator(IDENTITY, "TokenRequest").is_valid({**request, **change}))
        authorize = IDENTITY["paths"]["/api/attribution/v1/authorize"]["get"]
        params = {p["name"]: p for p in authorize["parameters"]}
        self.assertEqual(params["code_challenge_method"]["schema"], {"const": "S256"})
        self.assertTrue(params["state"]["required"])
        self.assertNotIn("user_id", params)
        for path, _, operation in operations(IDENTITY):
            if path.endswith(("/identity", "/access", "/companies", "/revoke")):
                self.assertEqual(operation["security"], [{"IdentityGrant": []}])
        cases = load("examples/authority-cases.json")
        self.assertEqual(len(cases["credential_classes"]), 7)
        self.assertEqual(len(cases["companies"]), 2)
        stores = {s for c in cases["companies"] for s in c["stores"]}
        self.assertEqual(len(stores), 4)
        self.assertEqual(cases["roles"]["member"], ["read"])
        self.assertEqual(cases["roles"]["owner"], cases["roles"]["admin"])
        self.assertEqual(cases["roles"]["none"], [])

    def test_attribution_contract_bounds_and_errors_are_declared_for_all_routes(self):
        for document in (API, IDENTITY):
            for path, method, operation in operations(document):
                with self.subTest(path=path, method=method):
                    self.assertGreater(operation["x-max-body-bytes"], 0)
                    self.assertLessEqual(operation["x-max-body-bytes"], 262144)
                    self.assertIn("429", operation["responses"])
                    self.assertIn("503", operation["responses"])
                    for parameter in operation.get("parameters", []):
                        if parameter["name"] == "limit":
                            self.assertEqual(parameter["schema"]["maximum"], 100)
        head = API["paths"]["/r/{code}"]["head"]["responses"]
        self.assertIn("204", head)
        self.assertNotIn("302", head)
        self.assertNotIn("content", head["204"])
        redirect = API["paths"]["/r/{code}"]["get"]["responses"]["302"]
        self.assertEqual(redirect["headers"]["Cache-Control"]["schema"], {"const": "no-store"})

    def test_attribution_contract_refund_business_identity_example_is_discriminating(self):
        ledger = load("examples/ledger.json")
        first, second = copy.deepcopy(ledger["refund_deliveries"])
        self.assertNotEqual(first.pop("event_id"), second.pop("event_id"))
        self.assertEqual(first, second)
        self.assertEqual(first["refund_id"], ledger["conflicting_refund"]["refund_id"])
        self.assertNotEqual(first["lines"], ledger["conflicting_refund"]["lines"])
        amount = sum(line["merchandise_amount"] for line in first["lines"])
        self.assertLess(2 * amount, ledger["paid"]["merchandise_amount"])
        self.assertEqual(ledger["expected"]["refund_facts"], 1)
        self.assertEqual(ledger["expected"]["refunded_merchandise"], amount)
        # This proves the fixture distinguishes double debit, not that runtime dedup works.

    def test_attribution_contract_two_ad_product_refund_ledger_reconciles(self):
        ledger = load("examples/ledger.json")
        paid, refund, expected = ledger["paid"], ledger["refund_deliveries"][0], ledger["expected"]
        chosen = max(ledger["clicks"], key=lambda click: click["clicked_at"])
        self.assertEqual((chosen["ad"], chosen["click_id"]),
                         (expected["attributed_ad"], expected["click_id"]))
        merchandise = sum(line["merchandise_amount"] for line in paid["lines"])
        refunded = sum(line["merchandise_amount"] for line in refund["lines"])
        self.assertEqual(merchandise, paid["merchandise_amount"])
        self.assertEqual(merchandise - refunded, expected["net_merchandise"])
        self.assertEqual(sum(line["quantity"] for line in paid["lines"]), expected["sold_units"])
        self.assertEqual(sum(line["returned_quantity"] for line in refund["lines"]), expected["returned_units"])
        self.assertEqual(paid["tax_amount"] - refund["tax_amount"], expected["net_tax"])
        self.assertEqual(paid["shipping_amount"] - refund["shipping_amount"], expected["net_shipping"])
        self.assertEqual(expected["net_merchandise"] + expected["net_tax"] + expected["net_shipping"], expected["net_total"])
        self.assertEqual(1 / len(ledger["clicks"]), expected["period_conversion_rate"])
        self.assertEqual(ledger["later_period_expected"]["net_merchandise"], -refunded)
        self.assertIsNone(ledger["later_period_expected"]["period_conversion_rate"])

    def test_attribution_contract_every_management_mutation_has_a_retry_identity(self):
        checked = 0
        for path, method, operation in operations(API):
            if method in ("get", "head") or not path.startswith("/v1/"):
                continue
            if not any("Session" in security for security in operation["security"]):
                continue
            checked += 1
            parameters = {p["name"]: p for p in operation["parameters"]}
            for name in ("Idempotency-Key", "Idempotency-Created-At"):
                self.assertTrue(parameters[name]["required"], (path, method, name))
            self.assertEqual(operation["x-retry-identity"], "management-receipt")
            self.assertIn("409", operation["responses"])
            self.assertIn("410", operation["responses"])
        self.assertGreaterEqual(checked, 17)
        write = IDENTITY["components"]["schemas"]["CompanyStatusWrite"]
        self.assertIn("expected_version", write["required"])

    def test_attribution_contract_lost_key_response_replays_metadata_without_secret(self):
        replay = load("examples/credential-replay.json")
        validator(API, "CredentialReplay").validate(replay)
        self.assertFalse(validator(API, "CredentialReplay").is_valid({**replay, "secret": "lost"}))
        self.assertFalse(validator(API, "CredentialReplay").is_valid({**replay, "secret_available": True}))
        for path in ("/v1/credentials", "/v1/credentials/{id}/rotate"):
            responses = API["paths"][path]["post"]["responses"]
            self.assertEqual(responses["200"]["content"]["application/json"]["schema"],
                             {"$ref": "#/components/schemas/CredentialReplay"})
            self.assertEqual(responses["201"]["content"]["application/json"]["schema"],
                             {"$ref": "#/components/schemas/IssuedCredential"})

    def test_attribution_contract_ad_product_update_preserves_absent_and_clear(self):
        document = load("attribution.openapi.json")
        schema = document["paths"]["/v1/ads/{id}"]["patch"]["requestBody"]["content"]["application/json"]["schema"]
        self.assertEqual(schema, {"$ref": "#/components/schemas/AdUpdate"})
        check = validator(document, "AdUpdate")
        for value in [{"name": "Renamed"}, {"advertised_product_id": "product-2"},
                      {"advertised_product_id": None}, {"enabled": False}]:
            self.assertTrue(check.is_valid(value), value)
        for value in [{}, {"name": None}, {"enabled": None},
                      {"advertised_product_id": ""}, {"advertised_product_id": "with space"},
                      {"advertised_product_id": "x" * 129}, {"store_id": "foreign"}]:
            self.assertFalse(check.is_valid(value), value)

    def test_attribution_contract_sdk_integrity_matches_pinned_asset(self):
        import base64
        import hashlib
        service = Path(__file__).resolve().parents[1]
        digest = base64.b64encode(hashlib.sha384((service / "assets/sdk-1.0.0.js").read_bytes()).digest()).decode()
        self.assertIn('integrity="sha384-' + digest + '"', (service / "WEBSITE.md").read_text())
        self.assertIn('/sdk/1.0.0.js', (service / "WEBSITE.md").read_text())

    def test_attribution_contract_pixel_retry_clock_and_explicit_head(self):
        pixel = API["paths"]["/pixel.gif"]
        parameters = {p["name"]: p for p in pixel["get"]["parameters"]}
        self.assertEqual(pixel["get"]["x-event-id-requires"], ["observed_at"])
        self.assertEqual(parameters["observed_at"]["schema"]["format"], "date-time")
        self.assertEqual(parameters["observed_at"]["schema"]["maxLength"], 40)
        self.assertIn("204", pixel["head"]["responses"])
        self.assertNotIn("200", pixel["head"]["responses"])
        self.assertNotIn("content", pixel["head"]["responses"]["204"])
        for route in ["/v1/events", "/v1/checkouts"]:
            self.assertIn("24", API["paths"][route]["post"]["description"])

    def test_attribution_contract_reports_distinguish_reasons_and_monetary_refunds(self):
        report = load("examples/analytics.json")
        validator(API, "Analytics").validate(report)
        group = report["groups"][0]
        self.assertEqual(sum(group["unattributed_reasons"].values()), group["unattributed_paid_orders"])
        self.assertEqual(group["unattributed_reasons"]["no_consent"], 1)
        financial = group["currencies"][0]
        self.assertEqual(financial["refunds"], 2)
        self.assertEqual(financial["returned_units"], 0)
        one_refund = copy.deepcopy(report)
        one_refund["groups"][0]["currencies"][0]["refunds"] = 1
        validator(API, "Analytics").validate(one_refund)
        self.assertNotEqual(report, one_refund)
        missing = copy.deepcopy(report)
        missing["groups"][0]["unattributed_reasons"]["no_consent"] = 0
        missing["groups"][0]["unattributed_reasons"]["missing"] = 1
        validator(API, "Analytics").validate(missing)
        self.assertNotEqual(report, missing)
        for field in ("unattributed_reasons",):
            broken = copy.deepcopy(report)
            del broken["groups"][0][field]
            self.assertFalse(validator(API, "Analytics").is_valid(broken))
        broken = copy.deepcopy(report)
        del broken["groups"][0]["currencies"][0]["refunds"]
        self.assertFalse(validator(API, "Analytics").is_valid(broken))


if __name__ == "__main__":
    if sys.argv[1:] == ["--list"]:
        names = unittest.defaultTestLoader.getTestCaseNames(AttributionContracts)
        if not names:
            raise SystemExit("No contract tests selected")
        for name in names:
            print(name)
        print(f"Selected {len(names)} contract tests")
    else:
        unittest.main(verbosity=2)
