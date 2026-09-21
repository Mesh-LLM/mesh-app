"""Noninteractive release packaging paths; never execute either binary."""
import importlib.util
import pathlib
import tempfile
import tarfile
import json
import unittest

spec = importlib.util.spec_from_file_location(
    "pack", pathlib.Path(__file__).with_name("package-app.py"))
pack = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pack)


def fixture(root):
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
    return tray, archive


class PackageAppTests(unittest.TestCase):
    def test_requires_pin_and_exactly_one_engine_provenance(self):
        with tempfile.TemporaryDirectory() as root:
            root = pathlib.Path(root)
            for kwargs in ({"archive_sha256": None, "engine_version": "0.76.2"},
                           {"archive_sha256": "b" * 64},
                           {"archive_sha256": "b" * 64, "engine_version": "0.76.2",
                            "engine_commit": "a" * 40},
                           {"archive_sha256": "b" * 64, "engine_commit": "short"}):
                with self.assertRaises(ValueError):
                    pack.package(root / "tray", root / "archive", root / "out",
                                 version="1.0.0", **kwargs)
                self.assertFalse((root / "out").exists())

    def test_release_engine_preserves_runtime_and_records_provenance(self):
        with tempfile.TemporaryDirectory() as root:
            root = pathlib.Path(root)
            tray, archive = fixture(root)
            out = root / "out"
            pack.package(tray, archive, out, version="1.2.3",
                         archive_sha256=pack.sha(archive), engine_version="0.76.2")
            manifest = json.loads((out / "SHA256.json").read_text())
            self.assertEqual(manifest["app_version"], "1.2.3")
            self.assertEqual(manifest["mesh_version"], "0.76.2")
            self.assertIsNone(manifest["mesh_source_commit"])
            self.assertEqual(manifest["engine_kind"], "official-release")
            self.assertEqual((out / "Mesh.app/Contents/Resources/engine/"
                              "native-runtimes/test/manifest.json").read_text(), "{}")

    def test_source_engine_records_commit(self):
        with tempfile.TemporaryDirectory() as root:
            root = pathlib.Path(root)
            tray, archive = fixture(root)
            out = root / "out"
            pack.package(tray, archive, out, version="1.2.3",
                         archive_sha256=pack.sha(archive), engine_commit="a" * 40)
            manifest = json.loads((out / "SHA256.json").read_text())
            self.assertEqual(manifest["mesh_source_commit"], "a" * 40)
            self.assertEqual(manifest["engine_kind"], "source")

    def test_checksum_mismatch_creates_nothing(self):
        with tempfile.TemporaryDirectory() as root:
            root = pathlib.Path(root)
            archive = root / "bundle.tar.gz"
            archive.write_bytes(b"wrong archive")
            with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                pack.package(root / "tray", archive, root / "out", version="1.0.0",
                             archive_sha256="b" * 64, engine_version="0.76.2")
            self.assertFalse((root / "out").exists())

    def test_existing_destination_is_never_replaced(self):
        with tempfile.TemporaryDirectory() as root:
            root = pathlib.Path(root)
            marker = root / "established"
            marker.write_bytes(b"preserve")
            with self.assertRaisesRegex(ValueError, "refusing replacement"):
                pack.package(root / "tray", root / "archive", root, version="1.0.0",
                             archive_sha256="b" * 64, engine_version="0.76.2")
            self.assertEqual(marker.read_bytes(), b"preserve")


if __name__ == "__main__":
    unittest.main()
