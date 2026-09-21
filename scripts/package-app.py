#!/usr/bin/env python3
"""Compose the release Mesh.app; never launch, sign, or touch a profile.

Unlike package-preview.py this is parameterized for CI: the caller states the
app version, the engine archive, and the archive digest it already verified
against the engine's published .sha256 sidecar (or computed itself for a
source-built engine). The script re-verifies the digest, records provenance in
SHA256.json, and leaves signing to the workflow.
"""
import argparse
import hashlib
import json
import pathlib
import plistlib
import re
import shutil
import subprocess
import tarfile

BUNDLE_ID = "com.mesh-llm.tray"
APP_NAME = "Mesh"


def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def package(tray, archive, destination, *, version, archive_sha256,
            engine_version=None, engine_commit=None):
    if not re.fullmatch(r"[0-9a-f]{64}", archive_sha256 or ""):
        raise ValueError("An archive SHA256 pin is required")
    if not re.fullmatch(r"[0-9A-Za-z][-+.0-9A-Za-z]*", version):
        raise ValueError("App version must be a plain version string")
    if engine_commit is not None and not re.fullmatch(r"[0-9a-f]{40}", engine_commit):
        raise ValueError("Engine commit must be a full lowercase Git SHA")
    if (engine_version is None) == (engine_commit is None):
        raise ValueError("State exactly one of engine version or engine commit")
    if destination.exists():
        raise ValueError("Destination already exists; refusing replacement")
    if sha(archive) != archive_sha256:
        raise ValueError("Engine archive checksum mismatch")
    if not tray.is_file():
        raise ValueError("Build target/release/mesh-tray first")
    destination.mkdir(parents=True)
    app = destination / f"{APP_NAME}.app"
    macos = app / "Contents/MacOS"
    macos.mkdir(parents=True)
    # Python's data filter rejects escaping paths/links and device files.
    with tarfile.open(archive) as bundle:
        bundle.extractall(destination / "runtime", filter="data")
    runtime = destination / "runtime/mesh-bundle"
    if not (runtime / "mesh-llm").is_file():
        raise ValueError("Engine archive does not contain mesh-bundle/mesh-llm")
    resources = app / "Contents/Resources/engine"
    resources.mkdir(parents=True)
    for path in runtime.iterdir():
        target = macos if path.name == "mesh-llm" else resources
        shutil.move(str(path), target / path.name)
    shutil.rmtree(destination / "runtime")
    shutil.copy2(tray, macos / "mesh-tray")
    with (app / "Contents/Info.plist").open("wb") as out:
        plistlib.dump({
            "CFBundleExecutable": "mesh-tray",
            "CFBundleIdentifier": BUNDLE_ID,
            "CFBundleName": APP_NAME,
            "CFBundlePackageType": "APPL",
            "CFBundleShortVersionString": version,
            "CFBundleVersion": version,
            "LSUIElement": True,
            "LSMinimumSystemVersion": "13.0",
        }, out)
    repo = pathlib.Path(__file__).resolve().parents[1]
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip()
    manifest = {"app_version": version,
                "tray_commit": head,
                "mesh_version": engine_version,
                "mesh_source_commit": engine_commit,
                "engine_kind": "official-release" if engine_version else "source",
                "archive_sha256": archive_sha256,
                "files": {}}
    for path in sorted(app.rglob("*")):
        if path.is_file():
            manifest["files"][str(path.relative_to(destination))] = sha(path)
    (destination / "SHA256.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(destination)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tray", type=pathlib.Path)
    parser.add_argument("archive", type=pathlib.Path)
    parser.add_argument("destination", type=pathlib.Path)
    parser.add_argument("--version", required=True)
    parser.add_argument("--archive-sha256", required=True)
    parser.add_argument("--engine-version")
    parser.add_argument("--engine-commit")
    args = parser.parse_args()
    package(args.tray.resolve(), args.archive.resolve(), args.destination.resolve(),
            version=args.version, archive_sha256=args.archive_sha256,
            engine_version=args.engine_version, engine_commit=args.engine_commit)
