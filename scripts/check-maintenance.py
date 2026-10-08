#!/usr/bin/env python3
"""Check workspace versions, local documentation links, and distributable contents."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tarfile
from urllib.parse import unquote

ROOT = Path(__file__).resolve().parents[1]
NAMES = {"solcompat", "solcompat-core", "solcompat-project"}
PRIVATE = {"PLAN.md", "REVIEW.md", "Solcompatplan.md", "sol_compat_PLAN.md", "MAINTAINABILITY_PLAN.md"}


def metadata():
    result = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--locked", "--offline", "--format-version", "1"],
        cwd=ROOT, capture_output=True, text=True, check=True,
    )
    document = json.loads(result.stdout)
    members = set(document["workspace_members"])
    packages = {p["name"]: p for p in document["packages"] if p["id"] in members}
    if set(packages) != NAMES:
        raise ValueError("Update the release inventory for the changed workspace members")
    versions = {p["version"] for p in packages.values()}
    if len(versions) != 1:
        raise ValueError("All three workspace packages must have the same release version")
    version = versions.pop()
    for package in packages.values():
        for dependency in package["dependencies"]:
            if dependency["name"] in NAMES and dependency["req"] != "^" + version:
                raise ValueError(f"{package['name']}: update {dependency['name']} version requirement to {version}")
    return version, packages


def documentation():
    files = [ROOT / name for name in ("README.md", "CONTRIBUTING.md", "CHANGELOG.md")]
    files.extend(sorted((ROOT / "docs").glob("*.md")))
    count = 0
    for path in files:
        for target in re.findall(r"!?\[[^\]]*\]\(([^)]+)\)", path.read_text()):
            target = target.strip().split()[0].strip("<>")
            if re.match(r"[a-zA-Z][a-zA-Z0-9+.-]*:", target) or target.startswith("#"):
                continue
            target = unquote(target.split("#", 1)[0])
            destination = (path.parent / target).resolve()
            if not destination.is_relative_to(ROOT) or not destination.exists():
                raise ValueError(f"{path.relative_to(ROOT)}: broken local link {target}")
            if destination.name in PRIVATE:
                raise ValueError(f"{path.relative_to(ROOT)}: public link to private plan {target}")
            count += 1
    return count


def check_private(names):
    for name in names:
        parts = Path(name).parts
        if any(part in PRIVATE or part in {".git", ".codex", ".agents"} for part in parts):
            raise ValueError(f"Private input in distribution: {name}")


def crate_contents(directory, version, packages):
    for name, package in packages.items():
        filename = f"{name}-{version}.crate"
        candidates = sorted(directory.rglob(filename))
        if not candidates:
            raise ValueError(f"No {filename} under {directory}")
        if len({hashlib.sha256(p.read_bytes()).digest() for p in candidates}) != 1:
            raise ValueError(f"Conflicting staged copies of {filename}")
        archive = candidates[0]
        prefix = f"{name}-{version}/"
        with tarfile.open(archive, "r:gz") as tar:
            contents = {m.name.removeprefix(prefix) for m in tar.getmembers() if m.isfile()}
        check_private(contents)
        base = Path(package["manifest_path"]).parent
        required = {str(p.relative_to(base)) for p in (base / "src").rglob("*.rs")}
        required.update({"Cargo.toml", "Cargo.lock", "LICENSE"})
        if name == "solcompat-core":
            required.update(str(p.relative_to(base)) for p in (base / "compatibility").rglob("*.json"))
        if name == "solcompat":
            required.update({"README.md", "CONTRIBUTING.md", "CHANGELOG.md"})
            required.update(str(p.relative_to(ROOT)) for folder in ("docs", "schemas") for p in (ROOT / folder).rglob("*") if p.is_file())
        if missing := required - contents:
            raise ValueError(f"{archive.name}: missing {sorted(missing)}")


def release_archive(path, version):
    with tarfile.open(path, "r:gz") as tar:
        members = tar.getmembers()
        roots = {Path(m.name).parts[0] for m in members}
        if len(roots) != 1 or not next(iter(roots)).startswith(f"solcompat-v{version}-"):
            raise ValueError("Release archive root does not match workspace version")
        root = next(iter(roots))
        contents = {m.name.removeprefix(root + "/") for m in members if m.isfile()}
        check_private(contents)
        required = {"solcompat", "LICENSE", "README.md", "CHANGELOG.md", "CONTRIBUTING.md", "RELEASE-MANIFEST.txt"}
        required.update(str(p.relative_to(ROOT)) for folder in ("docs", "schemas") for p in (ROOT / folder).rglob("*") if p.is_file())
        required.update(str(p.relative_to(ROOT / "crates/solcompat-core")) for p in (ROOT / "crates/solcompat-core/compatibility").rglob("*.json"))
        if missing := required - contents:
            raise ValueError(f"Release archive missing {sorted(missing)}")


def annotated_tag(tag):
    ref = "refs/tags/" + tag
    kind = subprocess.check_output(["git", "cat-file", "-t", ref], cwd=ROOT, text=True).strip()
    if kind != "tag":
        raise ValueError("Release tag must be annotated")
    commit = subprocess.check_output(["git", "rev-parse", ref + "^{commit}"], cwd=ROOT, text=True).strip()
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    if commit != head:
        raise ValueError("Release tag does not point to the checked-out commit")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", action="store_true", help="print validated workspace version only")
    parser.add_argument("--tag", help="validate an annotated release tag name against the workspace")
    parser.add_argument("--crate-dir", type=Path)
    parser.add_argument("--archive", type=Path)
    args = parser.parse_args()
    version, packages = metadata()
    if args.tag is not None and args.tag != "v" + version:
        raise ValueError(f"Tag {args.tag!r} does not match v{version}")
    if args.tag is not None:
        annotated_tag(args.tag)
    if args.version:
        print(version)
        return
    links = documentation()
    if args.crate_dir:
        crate_contents(args.crate_dir, version, packages)
    if args.archive:
        release_archive(args.archive, version)
    print(f"Workspace v{version}, {links} local documentation links, and requested distributions checked.")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, subprocess.CalledProcessError, tarfile.TarError) as error:
        raise SystemExit(str(error)) from error
