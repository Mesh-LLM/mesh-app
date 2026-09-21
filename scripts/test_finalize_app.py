"""Distribution hash ordering; inert fixtures only."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('finalize', Path(__file__).with_name('finalize-app.py'))
f = importlib.util.module_from_spec(spec)
spec.loader.exec_module(f)


class FinalizeTests(unittest.TestCase):
    def fixture(self, root):
        runtime = root / 'Mesh.app/Contents/Resources/engine/native-runtimes/metal'
        runtime.mkdir(parents=True)
        (runtime / 'lib.dylib').write_bytes(b'signed library')
        manifest = {'runtime': {'backend': 'metal', 'files': {'lib.dylib': 'old'},
                                'tools': {}, 'sha256': 'old archive', 'signature': 'old'}}
        (runtime / 'manifest.json').write_text(json.dumps(manifest))
        (runtime.parent.parent / 'product-manifest.json').write_text('{"upstream":true}')
        (root / 'SHA256.json').write_text('{"archive_sha256":"original", "files":{}}')
        return runtime

    def test_refresh_runtime_before_outer_and_final_after_stapling(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            runtime = self.fixture(root)
            f.finalize(root, 'runtime')
            data = json.loads((runtime / 'manifest.json').read_text())['runtime']
            self.assertEqual(data['files']['lib.dylib'], f.sha(runtime / 'lib.dylib'))
            self.assertNotIn('signature', data)
            self.assertTrue((root / 'upstream-metadata/product-manifest.json').exists())
            signature = root / 'Mesh.app/Contents/_CodeSignature/CodeResources'
            signature.parent.mkdir()
            signature.write_bytes(b'final outer signature')
            f.finalize(root, 'final')
            manifest = json.loads((root / 'SHA256.json').read_text())
            self.assertEqual(manifest['archive_sha256'], 'original')
            self.assertEqual(manifest['files'][str(signature.relative_to(root))], f.sha(signature))

    def test_rejects_missing_runtime(self):
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaisesRegex(ValueError, 'No native'):
                f.finalize(Path(tmp), 'runtime')

    def test_rejects_escaping_runtime_path(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            runtime = self.fixture(root)
            manifest = runtime / 'manifest.json'
            manifest.write_text(json.dumps({'runtime': {'backend': 'metal',
                                                       'files': {'../escape': 'old'}}}))
            with self.assertRaisesRegex(ValueError, 'escapes'):
                f.finalize(root, 'runtime')
