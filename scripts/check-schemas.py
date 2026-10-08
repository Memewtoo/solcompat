#!/usr/bin/env python3
"""Development-only schema checks against actual CLI reports; no network access."""
import json
from pathlib import Path
import subprocess

from jsonschema import Draft202012Validator, FormatChecker

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "target/debug/solcompat"


def validator(path):
    schema = json.loads((ROOT / path).read_text())
    Draft202012Validator.check_schema(schema)
    return Draft202012Validator(schema, format_checker=FormatChecker())


def main():
    dataset = validator("crates/solcompat-core/compatibility/schema/dataset.schema.json")
    dataset.validate(
        json.loads(
            (
                ROOT
                / "crates/solcompat-core/compatibility/records/rpc-read-contracts.json"
            ).read_text()
        )
    )
    reports = validator("schemas/report.schema.json")
    validator("schemas/target.schema.json")
    for fixture, expected in [
        ("rpc-v1-failure", 1), ("rpc-v1-pass", 0),
        ("rpc-unknown", 0), ("rpc-summary", 0),
        ("mixed-workspace", 1),
    ]:
        args = [str(BINARY), "check", "--path", str(ROOT / "fixtures" / fixture), "--format", "json"]
        completed = subprocess.run(args, capture_output=True, check=False)
        assert completed.returncode == expected, completed.stderr
        assert not completed.stderr, completed.stderr
        reports.validate(json.loads(completed.stdout))
        detailed = subprocess.run(args + ["--detailed"], capture_output=True, check=False)
        assert (detailed.stdout, detailed.returncode) == (completed.stdout, completed.returncode)
    for fixture, expected in (("rpc-v1-pass", 0), ("mixed-workspace", 1)):
        args = [str(BINARY), "upgrade", "--path", str(ROOT / "fixtures" / fixture), "--format", "json"]
        completed = subprocess.run(args, capture_output=True, check=False)
        assert completed.returncode == expected, completed.stderr
        assert not completed.stderr, completed.stderr
        reports.validate(json.loads(completed.stdout))
        detailed = subprocess.run(args + ["--detailed"], capture_output=True, check=False)
        assert (detailed.stdout, detailed.returncode) == (completed.stdout, completed.returncode)
    for args, expected in [
        (["inspect", "--path", "fixtures/rpc-v1-failure"], 0),
        (["check", "--path", "fixtures/rpc-v1-pass", "--target", "unsupported"], 2),
    ]:
        completed = subprocess.run([str(BINARY), *args, "--format", "json"], cwd=ROOT, capture_output=True, check=False)
        assert completed.returncode == expected, completed.stderr
        reports.validate(json.loads(completed.stdout))
    print("Dataset and CLI report schemas validated; compact/detailed JSON agrees.")


if __name__ == "__main__":
    main()
