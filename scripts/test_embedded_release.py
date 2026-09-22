"""Release source identity and final package contracts; inert fixtures only."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest


def load(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


inputs = load('release-inputs')
package = load('verify-embedded-package')


class ReleaseTests(unittest.TestCase):
    def fixture(self):
        rev = 'a' * 40
        manifest = {'dependencies': {name: {'git': inputs.REPO, 'rev': rev}
                                    for name in inputs.DEPENDENCIES}}
        lock = {'package': [{'name': name, 'source': f'git+{inputs.REPO}?rev={rev}#{rev}'}
                            for name in inputs.DEPENDENCIES]}
        return manifest, lock

    def test_single_revision(self):
        self.assertEqual(inputs.engine_revision(*self.fixture()), 'a' * 40)

    def test_rejects_mixed_or_mutable_sources(self):
        for update in ({'rev': 'main'}, {'rev': 'b' * 40}, {'git': 'https://other'},
                       {'path': '../engine'}, {'branch': 'main'}, {'tag': 'v1'}):
            manifest, lock = self.fixture()
            manifest['dependencies']['mesh-llm-sdk'].update(update)
            with self.assertRaises(ValueError):
                inputs.engine_revision(manifest, lock)

    def test_rejects_lock_drift_and_overrides(self):
        manifest, lock = self.fixture()
        lock['package'][0]['source'] += 'bad'
        with self.assertRaises(ValueError):
            inputs.engine_revision(manifest, lock)
        manifest, lock = self.fixture()
        manifest['patch'] = {'source': {}}
        with self.assertRaises(ValueError):
            inputs.engine_revision(manifest, lock)

    def test_final_package_detects_tampering_and_child_host(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            app = root / 'Mesh.app'
            macos = app / 'Contents/MacOS'
            macos.mkdir(parents=True)
            (macos / 'mesh-tray').write_bytes(b'inert tray')
            native = app / 'Contents/Resources/engine/native-runtimes/metal'
            native.mkdir(parents=True)
            (native / 'lib.dylib').write_bytes(b'inert library')
            (native / 'manifest.json').write_text(json.dumps({'runtime': {
                'backend': {'kind': 'metal'}, 'files': {'lib.dylib': package.sha(native / 'lib.dylib')}}}))
            metadata = {'engine_kind': 'embedded-sdk', 'files': {
                str(p.relative_to(root)): package.sha(p) for p in app.rglob('*') if p.is_file()}}
            (root / 'SHA256.json').write_text(json.dumps(metadata))
            package.verify(root)
            (native / 'lib.dylib').write_bytes(b'changed')
            with self.assertRaisesRegex(ValueError, 'hashes'):
                package.verify(root)
            (macos / 'mesh-llm').write_bytes(b'unwanted host')
            with self.assertRaisesRegex(ValueError, 'only embedded'):
                package.verify(root)

    def test_workflow_uses_embedded_producers_and_never_launches_app(self):
        workflow = (Path(__file__).parents[1] / '.github/workflows/release-macos.yml').read_text()
        self.assertIn('python3 scripts/release-inputs.py', workflow)
        self.assertIn('just release-runtime-build metal', workflow)
        self.assertIn('--embedded --engine-commit', workflow)
        self.assertIn('verify-host-dependencies.py', workflow)
        self.assertIn('verify-embedded-package.py', workflow)
        self.assertNotIn('inputs.engine', workflow)
        self.assertNotIn('Contents/MacOS/mesh-llm', workflow)
        self.assertNotIn('just release-bundle', workflow)
        self.assertIn("inputs.publish", workflow)

    def test_installer_is_separate_from_engineering_artifacts(self):
        workflow = (Path(__file__).parents[1] / '.github/workflows/release-macos.yml').read_text()
        self.assertIn('ln -s /Applications dmg-stage/Applications', workflow)
        self.assertIn('-srcfolder dmg-stage', workflow)
        self.assertIn('name: ${{ steps.dist.outputs.base }}-installer', workflow)
        self.assertIn('path: artifacts/*.dmg', workflow)
        self.assertIn('name: ${{ steps.dist.outputs.base }}-engineering', workflow)
        self.assertIn('!artifacts/*.dmg', workflow)
        publication = workflow.split('      - name: Publish release')[1].split('      - name: Clean')[0]
        self.assertNotIn('artifacts/*', publication)
        self.assertIn('.dmg.sha256', publication)
