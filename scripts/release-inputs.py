#!/usr/bin/env python3
"""Resolve the single reviewed engine revision; no network or credentials."""
import argparse
from pathlib import Path
import re
import tomllib

REPO = "https://github.com/Mesh-LLM/mesh-llm"
DEPENDENCIES = ("mesh-llm-sdk", "mesh-llm-host-runtime", "mesh-llm-identity")


def engine_revision(manifest, lock):
    deps = manifest["dependencies"]
    revisions = set()
    for name in DEPENDENCIES:
        dep = deps[name]
        if dep.get("git") != REPO or not re.fullmatch(r"[0-9a-f]{40}", dep.get("rev", "")):
            raise ValueError(f"{name} must use the official repository and full revision")
        if any(key in dep for key in ("path", "branch", "tag")):
            raise ValueError(f"{name} has an ambiguous source")
        revisions.add(dep["rev"])
    if len(revisions) != 1:
        raise ValueError("SDK, host and identity revisions differ")
    revision = revisions.pop()
    source = f"git+{REPO}?rev={revision}#{revision}"
    for name in DEPENDENCIES:
        matches = [p for p in lock["package"] if p["name"] == name]
        if len(matches) != 1 or matches[0].get("source") != source:
            raise ValueError(f"{name} lockfile does not match the manifest")
    if manifest.get("patch") or manifest.get("replace"):
        raise ValueError("Release does not support dependency source overrides")
    return revision


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path, default=Path("."), nargs="?")
    args = parser.parse_args()
    manifest = tomllib.loads((args.root / "Cargo.toml").read_text())
    lock = tomllib.loads((args.root / "Cargo.lock").read_text())
    print(engine_revision(manifest, lock))
