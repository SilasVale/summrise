#!/usr/bin/env python3
"""Offline fixture tests for the release finalizer's write order and receipts."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest


spec = importlib.util.spec_from_file_location(
    'finalize_release', Path(__file__).with_name('finalize-release.py')
)
finalizer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(finalizer)


class ReleaseFixture:
    def __init__(self, root):
        self.root = root
        self.events = []
        self.published = False
        self.native_mismatch = False
        self.public_mismatch = False
        self.revocations_present = False
        self.pointer_mismatch = False
        self.promote_error = False
        self.pointer = root / 'pointer.json'
        self.pointer.write_bytes(b'previous pointer')
        self.artifact_root = root / 'mirror'
        self.artifact_root.mkdir()
        self.config = {
            'schema': 1, 'go': True, 'repository': 'fixture/release',
            'source_sha': 'a' * 40, 'version': '2.0.987', 'channel': 'canary',
            'tag': 'v2.0.987', 'release_id': 1, 'ci_run_id': 2, 'installer_run_id': 3,
            'artifact_origin': finalizer.ORIGIN,
            'artifact_base': finalizer.ORIGIN + '/v2.0.987/canary',
            'proof_base': 'https://github.com/fixture/release/releases/download/v2.0.987',
            'min_version': '2.0.0', 'launcher_public_key': 'b' * 64,
            'release_public_key': 'c' * 64, 'release_source_root': str(root),
            'publication_lock': str(root / 'publish.lock'),
            'pointer_previous': finalizer.pin(self.pointer),
            'helper': {'path': str(finalizer.TRUSTED_HELPER),
                       'pin': {'sha256': finalizer.TRUSTED_HELPER_SHA}},
            'signer': {'path': str(finalizer.TRUSTED_CLI), 'pin': {
                'sha256': finalizer.TRUSTED_CLI_SHA, 'bytes': finalizer.TRUSTED_CLI_BYTES,
            }},
            'release_cli': {'sha256': 'd' * 64, 'bytes': 1},
        }
        mapping_receipt = root / 'mapping.json'
        mapping_receipt.write_text(json.dumps({
            'root_author': 'lead', 'confirmed': True,
            'artifact_root': str(self.artifact_root), 'artifact_origin': finalizer.ORIGIN,
        }))
        self.config['mapping'] = {
            'kind': 'nginx-static-v1', 'confirmed': True,
            'artifact_root': str(self.artifact_root), 'pointer_path': str(self.pointer),
            'pointer_url': 'https://console.saisi.online/installers/release-canary.json',
            'receipt_path': str(mapping_receipt), 'receipt_pin': finalizer.pin(mapping_receipt),
        }
        self.files = {
            name: (name + '\n').encode() for name in sorted(finalizer.names(self.config))
        }
        self.config['inventory_sha256'] = finalizer.sha(self.files['inventory.json'])
        self.config['expected_assets'] = [
            {'id': ident, 'name': name, 'size': len(raw), 'state': 'uploaded'}
            for ident, (name, raw) in enumerate(self.files.items(), 1)
        ]
        # Use the real MirrorClient proof gate and receipt check with a fixture backend.
        self.mirror = finalizer.MirrorClient.__new__(finalizer.MirrorClient)
        self.mirror.c = self.config
        self.mirror.backend = self

    def snapshot(self, config):
        self.events.append(('snapshot', 'public' if self.published else 'draft'))
        source = {'sha': config['source_sha']}
        workflow = {'head_sha': source['sha'], 'event': 'push', 'status': 'completed',
                    'conclusion': 'success'}
        return {
            'main': source, 'tag': source,
            'ci': dict(workflow, id=config['ci_run_id'], head_branch='main',
                       path='.github/workflows/ci.yml'),
            'installer': dict(workflow, id=config['installer_run_id'], head_branch=config['tag'],
                              path='.github/workflows/installer.yml'),
            'release': {'id': config['release_id'], 'tag_name': config['tag'],
                        'target_commitish': source['sha'], 'draft': not self.published,
                        'prerelease': self.published, 'assets': config['expected_assets']},
        }

    def download_asset(self, ident, size):
        return self.files[self.config['expected_assets'][ident - 1]['name']]

    def publish(self, config):
        self.events.append(('publish',))
        self.published = True
        return {'id': config['release_id'], 'draft': False}

    def public_asset(self, url, size):
        if not self.published:
            raise AssertionError('anonymous download attempted before publication')
        name = url.removeprefix(self.config['proof_base'] + '/')
        raw = self.files[name]
        return b'x' * len(raw) if self.public_mismatch else raw

    def verify(self, bundle, label):
        self.events.append(('verify', label))
        actual = {file.name: file.read_bytes() for file in bundle.iterdir()}
        if actual != self.files:
            raise finalizer.Refusal('bundle differs from fixture bytes')
        return {file.name: finalizer.pin(file) for file in bundle.iterdir()}

    def stage_verified(self, bundle, **kwargs):
        self.events.append(('stage', bundle.name))
        files = {}
        for source in bundle.iterdir():
            target = self.artifact_root / source.name
            with target.open('xb') as out:
                out.write(source.read_bytes())
            files[target.name] = finalizer.pin(target)
        return {'artifact_base': kwargs['artifact_base'],
                'inventory_sha256': kwargs['inventory_sha256'], 'files': files}

    def promote_pointer(self, document, **kwargs):
        self.events.append(('promote',))
        if finalizer.pin(self.pointer) != kwargs['expected_previous']:
            raise finalizer.Refusal('fixture pointer CAS differs')
        self.pointer.write_bytes(document.read_bytes())
        if self.promote_error:
            raise OSError('fixture promotion receipt write failed')
        return {'publication_receipt': kwargs['publication_receipt']}

    def transport(self, method, url, headers, body, limit):
        if method != 'GET':
            raise AssertionError(f'unexpected fixture HTTP method: {method}')
        if url == self.config['mapping']['pointer_url']:
            self.events.append(('pointer proof',))
            raw = self.pointer.read_bytes()
            return 200, {}, b'x' * len(raw) if self.pointer_mismatch else raw
        name = url.removeprefix(self.config['artifact_base'] + '/')
        if name == 'revocations.json':
            self.events.append(('revocations proof',))
            return (200 if self.revocations_present else 404), {}, b''
        if name not in self.files:
            raise AssertionError(f'unexpected fixture HTTP URL: {url}')
        raw = (self.artifact_root / name).read_bytes()
        return 200, {}, b'x' * len(raw) if self.native_mismatch else raw

    def run(self, phase='finalize'):
        return finalizer.run(self.config, self.root, self, self.mirror, self,
                             execute=True, phase=phase, transport=self.transport)

    def receipt(self, name):
        evidence, = self.root.glob('release-final-*')
        return json.loads((evidence / (name + '.json')).read_bytes())


class FinalizerTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.fixture = ReleaseFixture(Path(scratch.name).resolve())

    def test_native_proof_precedes_publication_and_pointer_is_last(self):
        f = self.fixture
        result = f.run()
        self.assertEqual(f.events, [
            ('snapshot', 'draft'), ('verify', 'draft'), ('snapshot', 'draft'),
            ('stage', 'draft'), ('verify', 'native'), ('revocations proof',),
            ('snapshot', 'draft'), ('publish',), ('verify', 'public'),
            ('snapshot', 'public'), ('promote',), ('pointer proof',),
        ])
        self.assertEqual(result['state'], 'PUBLISHED_DELIVERED_AND_POINTER_VERIFIED')
        self.assertTrue(result['pointer_last'])
        pins = f.receipt('result')['files']
        self.assertEqual(len(pins), 16)
        self.assertEqual(f.receipt('mirror-stage')['files'], pins)
        self.assertEqual(set(f.receipt('same-host-files')), set(pins))
        self.assertEqual(f.receipt('same-host-revocations')['status'], 404)
        self.assertFalse(f.receipt('publish')['draft'])
        self.assertEqual(f.receipt('pointer-promote')['publication_receipt'], {
            'state': 'PUBLIC_BUNDLE_VERIFIED', 'release_id': 1,
            'source_sha': 'a' * 40, 'inventory_sha256': f.config['inventory_sha256'],
        })
        self.assertEqual(f.receipt('pointer-public')['sha256'], finalizer.pin(f.pointer)['sha256'])

    def test_native_hash_mismatch_keeps_release_draft(self):
        f = self.fixture
        f.native_mismatch = True
        with self.assertRaisesRegex(finalizer.Refusal, 'same-host mirror HTTP bytes differ'):
            f.run()
        self.assertFalse(f.published)
        self.assertIs(f.receipt('failure')['pointer_unchanged'], True)
        self.assertEqual(f.pointer.read_bytes(), b'previous pointer')

    def test_revocations_must_be_absent_before_publication(self):
        f = self.fixture
        f.revocations_present = True
        with self.assertRaisesRegex(finalizer.Refusal, 'native optional revocations'):
            f.run()
        self.assertFalse(f.published)
        self.assertIs(f.receipt('failure')['pointer_unchanged'], True)

    def test_public_hash_mismatch_prevents_promotion(self):
        f = self.fixture
        f.public_mismatch = True
        with self.assertRaisesRegex(finalizer.Refusal, 'public GitHub bytes differ'):
            f.run()
        self.assertTrue(f.published)
        self.assertNotIn(('promote',), f.events)
        self.assertIs(f.receipt('failure')['pointer_unchanged'], True)
        self.assertEqual(f.pointer.read_bytes(), b'previous pointer')

    def test_pointer_https_mismatch_after_promotion_leaves_state_unknown(self):
        f = self.fixture
        f.pointer_mismatch = True
        with self.assertRaisesRegex(finalizer.Refusal, 'public channel pointer differs'):
            f.run()
        self.assertEqual(f.events[-2:], [('promote',), ('pointer proof',)])
        self.assertEqual(f.pointer.read_bytes(), f.files['release-2.0.987-canary.json'])
        self.assertIsNone(f.receipt('failure')['pointer_unchanged'])

    def test_error_within_promotion_leaves_pointer_state_unknown(self):
        f = self.fixture
        f.promote_error = True
        with self.assertRaisesRegex(finalizer.Refusal, 'fixture promotion receipt write failed'):
            f.run()
        self.assertEqual(f.events[-1], ('promote',))
        self.assertEqual(f.pointer.read_bytes(), f.files['release-2.0.987-canary.json'])
        self.assertIsNone(f.receipt('failure')['pointer_unchanged'])

    def test_draft_phase_leaves_release_mirror_and_pointer_untouched(self):
        f = self.fixture
        self.assertEqual(f.run(phase='draft')['state'], 'DRAFT_VERIFIED')
        self.assertEqual(f.events, [
            ('snapshot', 'draft'), ('verify', 'draft'), ('snapshot', 'draft'),
        ])
        self.assertEqual(list(f.artifact_root.iterdir()), [])
        self.assertEqual(f.pointer.read_bytes(), b'previous pointer')
        self.assertFalse(f.receipt('draft-verified')['published'])


if __name__ == '__main__':
    unittest.main(verbosity=2)
