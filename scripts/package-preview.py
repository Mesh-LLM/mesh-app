#!/usr/bin/env python3
"""Package only; never launch, sign, inspect credentials, or replace a profile."""
import argparse
import hashlib
import json
import pathlib
import plistlib
import re
import shutil
import subprocess
import tarfile

ARCHIVE_SHA256 = "a2e3c57ab8af03ca0815fa19ef8649852fba12a314d4fe75b977478c8524f63e"
HOST_SHA256 = "02c789bacf1360aed9b6024da11ece7b69acc81ca32e243c7a81bcea21bce12f"


def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def package(tray, archive, destination, *, source_commit=None,
            archive_sha256=None, host_sha256=None):
    # Source candidates must be explicitly pinned, never an implicit bypass of
    # the release checks. The caller supplies digests from the composed product.
    source_build = source_commit is not None
    if source_build:
        if not re.fullmatch(r"[0-9a-f]{40}", source_commit):
            raise ValueError("Source commit must be a full lowercase Git SHA")
        for digest in (archive_sha256, host_sha256):
            if not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest):
                raise ValueError("Source candidates require archive and host SHA256 pins")
    elif archive_sha256 is not None or host_sha256 is not None:
        raise ValueError("Custom digests require an explicit source commit")
    expected_archive = archive_sha256 if source_build else ARCHIVE_SHA256
    expected_host = host_sha256 if source_build else HOST_SHA256
    if destination.exists():
        raise ValueError("Destination already exists; refusing replacement")
    if sha(archive) != expected_archive:
        raise ValueError("Source archive checksum mismatch" if source_build else
                         "Expected official Mesh 0.76.2 macOS arm64 archive")
    if not tray.is_file():
        raise ValueError("Build target/release/mesh-tray first")
    destination.mkdir(parents=True)
    app = destination / "Mesh Candidate.app"
    macos = app / "Contents/MacOS"
    macos.mkdir(parents=True)
    # Python's data filter rejects escaping paths/links and device files.
    with tarfile.open(archive) as bundle:
        bundle.extractall(destination / "runtime", filter="data")
    runtime = destination / "runtime/mesh-bundle"
    if sha(runtime / "mesh-llm") != expected_host:
        raise ValueError("Released host checksum mismatch")
    for path in runtime.iterdir():
        shutil.move(str(path), macos / path.name)
    shutil.rmtree(destination / "runtime")
    shutil.copy2(tray, macos / "mesh-tray")
    with (app / "Contents/Info.plist").open("wb") as out:
        plistlib.dump({
            "CFBundleExecutable": "mesh-tray",
            "CFBundleIdentifier": "com.mesh-llm.tray.candidate",
            "CFBundleName": "Mesh Candidate",
            "CFBundlePackageType": "APPL",
            "CFBundleShortVersionString": "0.1.0",
            "CFBundleVersion": "1",
            "LSUIElement": True,
            "LSMinimumSystemVersion": "13.0",
        }, out)
    repo = pathlib.Path(__file__).resolve().parents[1]
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip()
    manifest = {"tray_commit": head,
                "mesh_version": None if source_build else "0.76.2",
                "mesh_source_commit": source_commit,
                "candidate_kind": "source" if source_build else "official-release",
                "archive_sha256": expected_archive, "host_sha256": expected_host,
                "files": {}}
    for path in sorted(app.rglob("*")):
        if path.is_file():
            manifest["files"][str(path.relative_to(destination))] = sha(path)
    (destination / "SHA256.json").write_text(json.dumps(manifest, indent=2) + "\n")
    guide = (repo / "HUMAN_TESTING.md").read_text()
    if source_build:
        guide = ("# Source-build candidate — NOT the official release trial\n\n"
                 f"Engine source commit: `{source_commit}`. Archive and host were "
                 "checked against caller-supplied pins, not official release pins. "
                 "These pins record the input; they do not attest its origin.\n\n"
                 "The release/version/artifact claims in the guide below do not "
                 "apply to this candidate. Do not launch alongside a live Mesh. "
                 "Restart-after-expiry remains an acceptance test, not a packaging "
                 "guarantee.\n\n---\n\n" + guide)
    (destination / "READ_ME_FIRST.md").write_text(guide)
    print(destination)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tray", type=pathlib.Path)
    parser.add_argument("archive", type=pathlib.Path)
    parser.add_argument("destination", type=pathlib.Path)
    parser.add_argument("--source-commit")
    parser.add_argument("--archive-sha256")
    parser.add_argument("--host-sha256")
    args = parser.parse_args()
    package(args.tray.resolve(), args.archive.resolve(), args.destination.resolve(),
            source_commit=args.source_commit, archive_sha256=args.archive_sha256,
            host_sha256=args.host_sha256)
