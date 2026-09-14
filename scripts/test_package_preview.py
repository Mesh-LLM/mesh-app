"""Noninteractive packaging failure paths; never execute either binary."""
import importlib.util
import pathlib
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "pack", pathlib.Path(__file__).with_name("package-preview.py"))
pack = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pack)


class PackageTests(unittest.TestCase):
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
