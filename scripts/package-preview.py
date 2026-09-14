#!/usr/bin/env python3
"""Package only; never launch, sign, inspect credentials, or replace a profile."""
import hashlib
import json
import pathlib
import plistlib
import shutil
import subprocess
import sys
import tarfile

ARCHIVE_SHA256 = "5bb00113682ba068862bfdf839f8519de8cde27bf0aaa6eb561727ec0392f7d6"
HOST_SHA256 = "79842e58af6b4258e17110099964e7aac2a9fab10b05dcd3a06dad22c3059c23"


def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def package(tray, archive, destination):
    if destination.exists():
        raise ValueError("Destination already exists; refusing replacement")
    if sha(archive) != ARCHIVE_SHA256:
        raise ValueError("Expected official Mesh 0.76.1 macOS arm64 archive")
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
    if sha(runtime / "mesh-llm") != HOST_SHA256:
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
    manifest = {"tray_commit": head, "mesh_version": "0.76.1",
                "archive_sha256": ARCHIVE_SHA256, "files": {}}
    for path in sorted(app.rglob("*")):
        if path.is_file():
            manifest["files"][str(path.relative_to(destination))] = sha(path)
    (destination / "SHA256.json").write_text(json.dumps(manifest, indent=2) + "\n")
    shutil.copy2(repo / "HUMAN_TESTING.md", destination / "READ_ME_FIRST.md")
    print(destination)


if __name__ == "__main__":
    if len(sys.argv) != 4:
        sys.exit("Usage: package-preview.py TRAY_BINARY OFFICIAL_ARCHIVE NEW_DESTINATION")
    package(*(pathlib.Path(arg).resolve() for arg in sys.argv[1:]))
