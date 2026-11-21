#!/usr/bin/env python3
"""Independent standard-library CSV oracle for the real HTTP export fixture."""
import csv
import io
import sys

reader = csv.DictReader(io.StringIO(sys.stdin.read()))
assert len(reader.fieldnames) == len(set(reader.fieldnames)), "CSV header names must be unique"
rows = list(reader)
assert all(None not in row for row in rows), "every value must have a column"
if sys.argv[1] == "currencies":
    assert len(rows) == 2
    assert rows[0]["attributed_paid_orders"] == "2"
    assert rows[1]["attributed_paid_orders"] == ""
    assert rows[0]["raw_clicks"] == "1" and rows[1]["raw_clicks"] == ""
    assert {row["currency"] for row in rows} == {"EUR", "USD"}
    assert all(row["currency_attributed_paid_orders"] == "1" for row in rows)
    assert all(row["currency_merchandise_amount"] == "5900" for row in rows)
    assert all(row["group_by"] == "overview" for row in rows)
else:
    assert sys.argv[1] == "products"
    assert len(rows) == 4
    groups = {name: [row for row in rows if row["group"] == name] for name in ("product-x", "product-y")}
    for name, group in groups.items():
        assert len(group) == 2
        assert group[0]["raw_clicks"] == "1" and group[1]["raw_clicks"] == ""
        assert group[0]["period_conversion_rate"] == ""
        assert group[0]["attributed_paid_orders"] == "2"
        assert group[1]["attributed_paid_orders"] == ""
        for row in group:
            assert row["group_by"] == "sold_product"
            assert "Contextual" in row["click_observation_basis"]
            assert "not purchased-product" in row["click_observation_basis"]
            assert "Matching product lines" in row["financial_basis"]
            assert "whole matching orders/refunds" in row["financial_basis"]
            assert row["nonadditive_measures"] == "Do not sum across product groups: clicks, observations, order/refund counts, tax/shipping"
            assert row["currency_merchandise_amount"] == ("2400" if name == "product-x" else "3500")
            assert row["currency_paid_orders"] == "1"
            assert row["currency_tax_amount"] == "1180"
            assert row["currency_shipping_amount"] == "500"
print("PASS: parsed CSV column identity, financial reconciliation and metric basis:", sys.argv[1])
