#!/usr/bin/env python3
"""Check signed embedded package layout and hashes without launching the app."""
import argparse
import hashlib
import json
from pathlib import Path


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def verify(root):
    metadata = json.loads((root / 'SHA256.json').read_text())
    if metadata['engine_kind'] != 'embedded-sdk':
        raise ValueError('Expected embedded SDK package')
    app = root / 'Mesh.app'
    if {p.name for p in (app / 'Contents/MacOS').iterdir()} != {'mesh-tray'}:
        raise ValueError('Expected only embedded tray executable')
    actual = {str(p.relative_to(root)): sha(p) for p in app.rglob('*') if p.is_file()}
    if actual != metadata['files']:
        raise ValueError('Final app file hashes differ')
    manifests = list(app.glob('Contents/Resources/engine/native-runtimes/*/manifest.json'))
    if len(manifests) != 1:
        raise ValueError('Expected exactly one Metal runtime')
    path = manifests[0]
    runtime = json.loads(path.read_text())['runtime']
    if runtime['backend']['kind'] != 'metal':
        raise ValueError('Expected Metal backend')
    if not runtime.get('files'):
        raise ValueError('Runtime has no checksummed payload')
    for name, digest in runtime['files'].items():
        payload = (path.parent / name).resolve()
        if not payload.is_relative_to(path.parent.resolve()) or sha(payload) != digest:
            raise ValueError('Native runtime file hash/path mismatch')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('root', type=Path)
    verify(parser.parse_args().root)
    print('Embedded package layout and final hashes verified; app not launched.')
