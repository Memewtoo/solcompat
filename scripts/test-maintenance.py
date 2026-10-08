#!/usr/bin/env python3
"""Regression checks for release metadata and distribution failure cases."""
import importlib.util
import io
import json
import subprocess
import sys
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True

spec = importlib.util.spec_from_file_location("maintenance", Path(__file__).with_name("check-maintenance.py"))
maintenance = importlib.util.module_from_spec(spec)
spec.loader.exec_module(maintenance)


class MaintenanceTests(unittest.TestCase):
    def test_metadata_rejects_uncoordinated_versions_and_requirements(self):
        packages = [dict(name=name, id=name, version="0.2.0", dependencies=[]) for name in maintenance.NAMES]
        document = dict(workspace_members=list(maintenance.NAMES), packages=packages)
        response = type("Response", (), {"stdout": json.dumps(document)})()
        with patch.object(maintenance.subprocess, "run", return_value=response):
            self.assertEqual(maintenance.metadata()[0], "0.2.0")
        packages[0]["version"] = "0.1.0"
        response.stdout = json.dumps(document)
        with patch.object(maintenance.subprocess, "run", return_value=response), self.assertRaisesRegex(ValueError, "same release version"):
            maintenance.metadata()
        packages[0]["version"] = "0.2.0"
        packages[0]["dependencies"] = [dict(name="solcompat-core", req="^0.1.0")]
        response.stdout = json.dumps(document)
        with patch.object(maintenance.subprocess, "run", return_value=response), self.assertRaisesRegex(ValueError, "version requirement"):
            maintenance.metadata()

    def test_broken_and_private_documentation_links_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "docs").mkdir()
            for name in ("README.md", "CONTRIBUTING.md", "CHANGELOG.md"):
                (root / name).write_text("")
            with patch.object(maintenance, "ROOT", root):
                (root / "README.md").write_text("[missing](docs/missing.md)")
                with self.assertRaisesRegex(ValueError, "broken local link"):
                    maintenance.documentation()
                (root / "PLAN.md").write_text("")
                (root / "README.md").write_text("[private](PLAN.md)")
                with self.assertRaisesRegex(ValueError, "private plan"):
                    maintenance.documentation()

    def test_archive_wrong_version_and_private_inputs_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "release.tar.gz"
            for member, expected in [("solcompat-v0.1.0-host/LICENSE", "root"), ("solcompat-v0.2.0-host/PLAN.md", "Private")]:
                with tarfile.open(archive, "w:gz") as tar:
                    info = tarfile.TarInfo(member)
                    info.size = 1
                    tar.addfile(info, io.BytesIO(b"x"))
                with self.assertRaisesRegex(ValueError, expected):
                    maintenance.release_archive(archive, "0.2.0")

    def test_release_tag_must_be_annotated_and_point_to_head(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            def git(*args):
                subprocess.run(["git", *args], cwd=root, check=True, capture_output=True)
            git("init", "--quiet")
            git("config", "user.name", "Release test")
            git("config", "user.email", "test@example.invalid")
            git("commit", "--allow-empty", "-m", "first")
            git("tag", "lightweight")
            git("tag", "-a", "v0.2.0", "-m", "release")
            with patch.object(maintenance, "ROOT", root):
                maintenance.annotated_tag("v0.2.0")
                with self.assertRaisesRegex(ValueError, "annotated"):
                    maintenance.annotated_tag("lightweight")
                git("commit", "--allow-empty", "-m", "second")
                with self.assertRaisesRegex(ValueError, "checked-out commit"):
                    maintenance.annotated_tag("v0.2.0")

    def test_staged_crates_include_current_modules_and_exclude_private_plans(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "src").mkdir()
            (root / "src/new_module.rs").write_text("")
            staging = root / "package/tmp-crate"
            staging.mkdir(parents=True)
            archive = staging / "solcompat-project-0.2.0.crate"
            packages = {"solcompat-project": {"manifest_path": str(root / "Cargo.toml")}}
            def write(names):
                with tarfile.open(archive, "w:gz") as tar:
                    for name in names:
                        info = tarfile.TarInfo("solcompat-project-0.2.0/" + name)
                        info.size = 1
                        tar.addfile(info, io.BytesIO(b"x"))
            required = ["Cargo.toml", "Cargo.lock", "LICENSE"]
            write(required)
            with self.assertRaisesRegex(ValueError, "new_module"):
                maintenance.crate_contents(root / "package", "0.2.0", packages)
            write(required + ["src/new_module.rs"])
            maintenance.crate_contents(root / "package", "0.2.0", packages)
            write(required + ["src/new_module.rs", "PLAN.md"])
            with self.assertRaisesRegex(ValueError, "Private"):
                maintenance.crate_contents(root / "package", "0.2.0", packages)


if __name__ == "__main__":
    unittest.main()
