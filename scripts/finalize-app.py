#!/usr/bin/env python3
"""Refresh distribution hashes after signing; retain original input provenance.

runtime: after nested signing, before outer signing. final: after stapling.
The redistributed engine is a new app-distribution artifact, not the original
upstream attested product. Keep original metadata outside the app as evidence.
"""
import argparse
import hashlib
import json
from pathlib import Path


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def finalize(root, phase):
    app = root / 'Mesh.app'
    resources = app / 'Contents/Resources/engine'
    if phase == 'runtime':
        evidence = root / 'upstream-metadata'
        evidence.mkdir(exist_ok=True)
        for path in resources.glob('*.json'):
            path.rename(evidence / path.name)
        manifests = list(resources.glob('native-runtimes/*/manifest.json'))
        if not manifests:
            raise ValueError('No native runtime manifest')
        for path in manifests:
            data = json.loads(path.read_text())
            runtime = data['runtime']
            if runtime['backend'].get('kind') != 'metal':
                raise ValueError('macOS app requires Metal runtime')
            (evidence / (path.parent.name + '.json')).write_text(path.read_text())
            for field in ('files', 'tools'):
                for name in runtime.get(field, {}):
                    payload = (path.parent / name).resolve()
                    if not payload.is_relative_to(path.parent.resolve()):
                        raise ValueError('Runtime path escapes bundle')
                    runtime[field][name] = sha(payload)
            for field in ('signature', 'sha256', 'url'):
                runtime.pop(field, None)
            path.write_text(json.dumps(data, indent=2) + '\n')
    else:
        path = root / 'SHA256.json'
        manifest = json.loads(path.read_text())
        manifest['distribution'] = 'mesh-app-developer-id'
        manifest['files'] = {str(p.relative_to(root)): sha(p)
                             for p in sorted(app.rglob('*')) if p.is_file()}
        path.write_text(json.dumps(manifest, indent=2) + '\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('root', type=Path)
    parser.add_argument('phase', choices=['runtime', 'final'])
    args = parser.parse_args()
    finalize(args.root, args.phase)
