"""Noninteractive packaging failure paths; never execute either binary."""
import importlib.util
import pathlib
import tempfile
import tarfile
import json
import unittest

spec = importlib.util.spec_from_file_location(
    "pack", pathlib.Path(__file__).with_name("package-preview.py"))
pack = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pack)


class PackageTests(unittest.TestCase):
    def test_source_requires_complete_pins(self):
        with tempfile.TemporaryDirectory() as root:
            root = pathlib.Path(root)
            for kwargs in ({"source_commit": "short"},
                           {"source_commit": "a" * 40},
                           {"archive_sha256": "b" * 64}):
                with self.assertRaises(ValueError):
                    pack.package(root / "tray", root / "archive", root / "out", **kwargs)
                self.assertFalse((root / "out").exists())

    def test_source_candidate_preserves_runtime_and_records_pins(self):
        with tempfile.TemporaryDirectory() as root:
            root = pathlib.Path(root)
            runtime = root / "mesh-bundle"
            native = runtime / "native-runtimes/test"
            native.mkdir(parents=True)
            (runtime / "mesh-llm").write_bytes(b"inert host fixture")
            (native / "manifest.json").write_text("{}")
            tray = root / "tray"
            tray.write_bytes(b"inert tray fixture")
            archive = root / "bundle.tar.gz"
            with tarfile.open(archive, "w:gz") as bundle:
                bundle.add(runtime, arcname="mesh-bundle")
            out = root / "out"
            pack.package(tray, archive, out, source_commit="a" * 40,
                         archive_sha256=pack.sha(archive),
                         host_sha256=pack.sha(runtime / "mesh-llm"))
            manifest = json.loads((out / "SHA256.json").read_text())
            self.assertEqual(manifest["mesh_source_commit"], "a" * 40)
            self.assertIsNone(manifest["mesh_version"])
            self.assertEqual(manifest["candidate_kind"], "source")
            self.assertEqual((out / "Mesh Candidate.app/Contents/MacOS/"
                              "native-runtimes/test/manifest.json").read_text(), "{}")
            self.assertFalse((out / "READ_ME_FIRST.md").exists())

    def test_source_archive_checksum_mismatch_creates_nothing(self):
        with tempfile.TemporaryDirectory() as root:
            root = pathlib.Path(root)
            archive = root / "bundle.tar.gz"
            archive.write_bytes(b"wrong source archive")
            with self.assertRaisesRegex(ValueError, "Source archive checksum mismatch"):
                pack.package(root / "tray", archive, root / "out",
                             source_commit="a" * 40, archive_sha256="b" * 64,
                             host_sha256="c" * 64)
            self.assertFalse((root / "out").exists())

    def test_invalid_archive_creates_nothing(self):
        with tempfile.TemporaryDirectory() as root:
            root = pathlib.Path(root)
            archive = root / "bad.tar.gz"
            archive.write_bytes(b"not official")
            with self.assertRaisesRegex(ValueError, "official"):
                pack.package(root / "tray", archive, root / "out")
            self.assertFalse((root / "out").exists())

    def test_existing_destination_is_never_replaced(self):
        with tempfile.TemporaryDirectory() as root:
            root = pathlib.Path(root)
            marker = root / "established"
            marker.write_bytes(b"preserve")
            with self.assertRaisesRegex(ValueError, "refusing replacement"):
                pack.package(root / "tray", root / "archive", root)
            self.assertEqual(marker.read_bytes(), b"preserve")


if __name__ == "__main__":
    unittest.main()
