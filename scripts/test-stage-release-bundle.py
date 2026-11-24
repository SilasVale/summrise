#!/usr/bin/env python3
"""Offline integration tests using an explicitly supplied real release signer."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import types
import unittest

HERE = Path(__file__).resolve().parent
HELPER = HERE / 'stage-release-bundle.py'
SIGNER = None
RUNTIME_ROOT = None
LEGACY_SIGNER = None
# The release sources that built the trusted CLI are recorded in the bundle. The tests use a
# synthetic tree with the same files so the pins are hers to break on purpose; a real bundle is
# staged against the actual device/release tree. The set is the helper's own RELEASE_SOURCE_FILES:
# the five release files whose bytes decide what the verifier accepts, including
# `src/document.rs`, the module that implements document::verify_at, the policy freshness check
# and the two inner signatures. The sibling `Cargo.lock` is pinned beside them.
SOURCE_FILES = ('src/verify.rs', 'src/lib.rs', 'src/document.rs',
                'src/bin/summrise-release.rs', 'tests/cli.rs')
EXPECTED_SOURCE_FILES = SOURCE_FILES
EXPECTED_SOURCE_PIN_NAMES = set(SOURCE_FILES) | {'Cargo.lock'}


def load_module(path, name):
    """Load a sibling module by path so the test reads the real delivered bytes."""
    spec = importlib.util.spec_from_file_location(name, Path(path))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def setUpModule():
    global SIGNER, RUNTIME_ROOT
    value = os.environ.get('SUMMRISE_RELEASE_TEST_SIGNER', '')
    candidate = Path(value)
    if not value or not candidate.is_absolute() or not candidate.is_file() or not os.access(candidate, os.X_OK):
        raise AssertionError('SUMMRISE_RELEASE_TEST_SIGNER must name an absolute existing executable; tests cannot skip it')
    SIGNER = candidate.resolve()
    selected = os.environ.get('SUMMRISE_RELEASE_TEST_ROOT')
    base = Path(selected) if selected else Path(tempfile.gettempdir())
    if not base.is_absolute():
        raise AssertionError('SUMMRISE_RELEASE_TEST_ROOT must be absolute')
    base = base.resolve()
    if base == HERE or HERE in base.parents:
        raise AssertionError('release test runtime root must be outside the source directory')
    base.mkdir(mode=0o700, parents=True, exist_ok=True)
    RUNTIME_ROOT = Path(tempfile.mkdtemp(prefix='summrise-release-tests-', dir=base))
    RUNTIME_ROOT.chmod(0o700)
    global LEGACY_SIGNER
    legacy = os.environ.get('SUMMRISE_RELEASE_TEST_LEGACY_SIGNER', '')
    if legacy:
        candidate = Path(legacy)
        if not candidate.is_absolute() or not candidate.is_file() or not os.access(candidate, os.X_OK):
            raise AssertionError('SUMMRISE_RELEASE_TEST_LEGACY_SIGNER must name an absolute existing executable')
        LEGACY_SIGNER = candidate.resolve()
    else:
        # The repository carries no old CLI binary, so the legacy case runs against a stand-in
        # whose bytes differ from the recorded CLI. The trusted signer above is still mandatory.
        LEGACY_SIGNER = None
    print('release tests runtime: ' + str(RUNTIME_ROOT), flush=True)
VERSION, CHANNEL, SOURCE = '2.0.987', 'canary', 'a' * 40
MIRROR = 'https://example.invalid/releases'
ROLES = ('launch', 'device', 'console', 'shell', 'setup')
NAMES = {role: (f'SummriseSetup-{VERSION}-{CHANNEL}.exe' if role == 'setup' else
                f'summrise-{role}-{VERSION}-{CHANNEL}.exe') for role in ROLES}
DOC = f'release-{VERSION}-{CHANNEL}.json'

def sidecar(name):
    return str(Path(name).with_suffix('.sidecar.json'))

def record(path):
    data = path.read_bytes()
    return {'sha256': hashlib.sha256(data).hexdigest(), 'bytes': len(data)}

class Bundle(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix=self._testMethodName + '-', dir=RUNTIME_ROOT))
        self.out = self.root / 'bundle'
        self.inputs = {}
        for role in ROLES:
            path = self.root / (role + ' fixture.exe')
            path.write_bytes(b'MZ\x00public test fixture bytes: ' + role.encode())
            self.inputs[role] = path
        # Public deterministic test material, never production signing keys.
        self.seeds, self.public = {}, {}
        for role, data in [('launcher', bytes(range(32))), ('release', bytes(reversed(range(32))))]:
            seed = self.root / (role + '.seed')
            seed.write_text(data.hex())
            seed.chmod(0o600)
            self.seeds[role] = seed
            result = subprocess.run([str(SIGNER), 'pubkey', '--key-file', str(seed)],
                                    capture_output=True, text=True, check=True)
            self.public[role] = result.stdout.strip()
        self.sequence = 0
        # A synthetic release source tree with the same five recorded files.
        self.source_root = self.root / 'release'
        (self.source_root / 'src' / 'bin').mkdir(parents=True)
        (self.source_root / 'tests').mkdir()
        for name in SOURCE_FILES:
            path = self.source_root / name
            path.write_text('// ' + name + ' public test source bytes\n')
        (self.root / 'Cargo.lock').write_text('# public test lockfile bytes\n')

    def call(self, mode, expected=0, **changes):
        opts = {'signer': SIGNER, 'out': self.out, 'version': VERSION, 'channel': CHANNEL,
                'source-sha': SOURCE, 'mirror': MIRROR, 'launcher-pubkey': self.public['launcher'],
                'release-pubkey': self.public['release'],
                'release-source-root': self.source_root}
        if mode == 'stage':
            opts.update(self.inputs)
            opts.update({'launcher-seed': self.seeds['launcher'], 'release-seed': self.seeds['release']})
        opts.update(changes)
        argv = [sys.executable, str(HELPER), mode]
        for key, value in opts.items():
            argv.extend(['--' + key, str(value)])
        result = subprocess.run(argv, capture_output=True, text=True, cwd=self.root)
        label = f'{self.sequence:02d}-{mode}'
        self.sequence += 1
        (self.root / (label + '.exit')).write_text(str(result.returncode) + '\n')
        (self.root / (label + '.stdout.raw')).write_text(result.stdout)
        (self.root / (label + '.stderr.raw')).write_text(result.stderr)
        if expected == 'failure':
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertFalse((self.out / 'READY').exists())
        else:
            self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
        return result

    def rewrite_inventory(self, mutate):
        path = self.out / 'inventory.json'
        inventory = json.loads(path.read_text())
        mutate(inventory)
        path.write_text(json.dumps(inventory, sort_keys=True) + '\n')
        subprocess.run([str(SIGNER), 'sidecar', '--key-file', str(self.seeds['release']),
                        '--artifact', str(path), '--version', VERSION, '--channel', CHANNEL,
                        '--out', str(self.out / 'inventory.sidecar.json')],
                       check=True, capture_output=True)
        self.refresh_sums()

    def resign_inventory(self):
        inventory = json.loads((self.out / 'inventory.json').read_text())
        inventory['files'] = {name: record(self.out / name) for name in inventory['files']}
        (self.out / 'inventory.json').write_text(json.dumps(inventory, sort_keys=True) + '\n')
        subprocess.run([str(SIGNER), 'sidecar', '--key-file', str(self.seeds['release']),
                        '--artifact', str(self.out / 'inventory.json'), '--version', VERSION,
                        '--channel', CHANNEL, '--out', str(self.out / 'inventory.sidecar.json')],
                       check=True, capture_output=True)
        self.refresh_sums()

    def refresh_sums(self):
        names = set(NAMES.values()) | {sidecar(n) for n in NAMES.values()} | {
            DOC, sidecar(DOC), 'inventory.json', 'inventory.sidecar.json'}
        text = ''.join(f"{record(self.out / n)['sha256']}  {n}\n" for n in sorted(names))
        (self.out / 'SHA256SUMS').write_text(text)
        # An attacker can recompute both unsigned checksums and the unsigned READY digest.
        if (self.out / 'READY').exists():
            (self.out / 'READY').write_text(record(self.out / 'SHA256SUMS')['sha256'] + '\n')

    def test_complete_bundle_has_signed_source_inventory_and_checksum_closure(self):
        self.call('stage')
        self.call('verify')
        files = set(NAMES.values()) | {sidecar(n) for n in NAMES.values()} | {DOC, sidecar(DOC)}
        inventory = json.loads((self.out / 'inventory.json').read_text())
        self.assertEqual(inventory['sourceSha'], SOURCE)
        self.assertEqual(inventory['version'], VERSION)
        self.assertEqual(inventory['channel'], CHANNEL)
        self.assertEqual(inventory['artifacts'], NAMES)
        import hashlib as _hashlib
        self.assertEqual(inventory['tooling']['cli']['sha256'], _hashlib.sha256(SIGNER.read_bytes()).hexdigest())
        self.assertEqual(inventory['tooling']['cli']['bytes'], SIGNER.stat().st_size)
        self.assertEqual(set(inventory['tooling']['source']), set(SOURCE_FILES) | {'Cargo.lock'})
        self.assertEqual(set(inventory['files']), files)
        for name in files:
            self.assertEqual(inventory['files'][name], record(self.out / name))
        immutable = files | {'inventory.json', 'inventory.sidecar.json'}
        self.assertEqual(set(p.name for p in self.out.iterdir()), immutable | {'SHA256SUMS', 'READY'})
        expected_sums = ''.join(f"{record(self.out / n)['sha256']}  {n}\n" for n in sorted(immutable))
        self.assertEqual((self.out / 'SHA256SUMS').read_text(), expected_sums)
        self.assertEqual((self.out / 'READY').read_text(), record(self.out / 'SHA256SUMS')['sha256'] + '\n')
        doc = json.loads((self.out / DOC).read_text())
        self.assertEqual(doc['sha256'], record(self.inputs['device'])['sha256'])
        self.assertEqual(doc['artifactUrl'], MIRROR + '/' + NAMES['device'])
        for role in ROLES:
            self.assertEqual((self.out / NAMES[role]).read_bytes(), self.inputs[role].read_bytes())
        wrong = subprocess.run([str(SIGNER), 'verify-artifact', '--artifact', str(self.out / NAMES['device']),
                                '--sidecar', str(self.out / sidecar(NAMES['device'])), '--version', VERSION,
                                '--channel', CHANNEL, '--public-key', self.public['release']], capture_output=True)
        self.assertNotEqual(wrong.returncode, 0, 'release key must not authenticate launcher bootstrap')

    def test_missing_console_refuses_before_ready(self):
        self.call('stage', expected='failure', console=self.root / 'missing-console.exe')

    def test_missing_setup_refuses_before_ready(self):
        self.call('stage', expected='failure', setup=self.root / 'missing-setup.exe')

    def test_wrong_seed_public_key_pair_refuses_before_any_output(self):
        self.call('stage', expected='failure', **{'release-pubkey': self.public['launcher']})
        self.assertFalse(self.out.exists())

    def test_existing_destination_is_never_overwritten(self):
        self.out.mkdir()
        sentinel = self.out / 'keep.txt'
        sentinel.write_bytes(b'owned bytes')
        self.call('stage', expected='failure')
        self.assertEqual(sentinel.read_bytes(), b'owned bytes')
        self.assertEqual(list(self.out.iterdir()), [sentinel])

    def test_same_input_for_two_roles_is_refused(self):
        self.call('stage', expected='failure', console=self.inputs['device'])

    def test_tampered_console_invalidates_ready_and_preserves_failed_bytes(self):
        self.call('stage')
        path = self.out / NAMES['console']
        path.write_bytes(b'MZchanged bytes')
        self.call('verify', expected='failure')
        self.assertEqual(path.read_bytes(), b'MZchanged bytes')

    def test_wrong_expected_source_version_channel_or_key_refuses(self):
        for changes in [{'source-sha': 'b' * 40}, {'version': '2.0.988'}, {'channel': 'stable'},
                        {'release-pubkey': self.public['launcher']}, {'launcher-pubkey': self.public['release']}]:
            with self.subTest(changes=changes):
                target = self.root / ('bundle-' + str(self.sequence))
                self.call('stage', out=target)
                old_out, self.out = self.out, target
                self.call('verify', expected='failure', **changes)
                self.out = old_out

    def test_bad_artifact_signature_fails_even_with_resigned_inventory_and_checksums(self):
        self.call('stage')
        path = self.out / sidecar(NAMES['console'])
        data = json.loads(path.read_text())
        data['signature'] = '00' * 64
        path.write_text(json.dumps(data))
        self.resign_inventory()
        result = self.call('verify', expected='failure')
        self.assertIn('verify-artifact', result.stderr)

    def corrupt_inner_signature_and_resign_outer(self, field):
        self.call('stage')
        path = self.out / DOC
        document = json.loads(path.read_text())
        document[field] = '00' * 64
        path.write_text(json.dumps(document) + '\n')
        # Real release-key signatures authenticate the changed document and inventory;
        # valid outer signatures must not substitute for either native inner signature.
        subprocess.run([str(SIGNER), 'sidecar', '--key-file', str(self.seeds['release']),
                        '--artifact', str(path), '--version', VERSION, '--channel', CHANNEL,
                        '--out', str(self.out / sidecar(DOC))], check=True, capture_output=True)
        self.resign_inventory()
        self.assertTrue((self.out / 'READY').is_file())
        result = self.call('verify', expected='failure')
        self.assertIn('verify-document', result.stderr)
        self.assertFalse((self.out / 'READY').exists())
        self.assertEqual(json.loads(path.read_text())[field], '00' * 64)

    def test_bad_inner_policy_signature_refuses_even_with_valid_outer_signatures(self):
        self.corrupt_inner_signature_and_resign_outer('signature')

    def test_bad_inner_artifact_signature_refuses_even_with_valid_outer_signatures(self):
        self.corrupt_inner_signature_and_resign_outer('artifactSignature')

    def test_missing_release_source_pin_refuses_without_ready(self):
        self.call('stage')
        missing = self.source_root / 'src' / 'verify.rs'
        missing.unlink()
        result = self.call('verify', expected='failure')
        self.assertIn('release source pin is missing', result.stderr)
        self.assertFalse((self.out / 'READY').exists())

    def test_recorded_cli_hash_must_match_the_cli_that_verifies(self):
        self.call('stage')
        other = self.root / 'other-summrise-release'
        other.write_bytes(SIGNER.read_bytes() + b'\x00')
        other.chmod(0o755)
        result = self.call('verify', expected='failure', signer=other)
        self.assertIn('recorded tooling pins differ', result.stderr)
        self.assertFalse((self.out / 'READY').exists())

    def test_rewritten_recorded_cli_hash_refuses(self):
        self.call('stage')

        def mutate(inventory):
            inventory['tooling']['cli']['sha256'] = '00' * 32

        self.rewrite_inventory(mutate)
        result = self.call('verify', expected='failure')
        self.assertIn('recorded tooling pins differ', result.stderr)
        self.assertFalse((self.out / 'READY').exists())

    def test_changed_release_source_pin_refuses(self):
        self.call('stage')
        (self.source_root / 'src' / 'lib.rs').write_text('// changed public test source bytes\n')
        result = self.call('verify', expected='failure')
        self.assertIn('recorded tooling pins differ', result.stderr)
        self.assertFalse((self.out / 'READY').exists())

    def test_legacy_cli_without_verify_document_is_refused(self):
        self.call('stage')
        if LEGACY_SIGNER is not None:
            # The old CLI really has no verify-document: a bundle cannot be verified by it at all.
            probe = subprocess.run([str(LEGACY_SIGNER), 'verify-document', '--document', str(self.out / DOC)],
                                   capture_output=True, text=True)
            self.assertNotEqual(probe.returncode, 0, 'the legacy CLI must not implement verify-document')
            signer = LEGACY_SIGNER
        else:
            # The repository carries no old CLI binary. A stand-in whose bytes differ from the
            # recorded CLI stands in for one: the helper refuses it by name, before it ever calls
            # the signer, so the refusal cannot depend on what that binary can do.
            signer = self.root / 'stand-in-summrise-release'
            signer.write_bytes(SIGNER.read_bytes() + b'\x00')
            signer.chmod(0o755)
        result = self.call('verify', expected='failure', signer=signer)
        self.assertIn('recorded tooling pins differ', result.stderr)
        self.assertFalse((self.out / 'READY').exists())

    def test_unsigned_inventory_source_change_is_rejected(self):
        self.call('stage')
        path = self.out / 'inventory.json'
        data = json.loads(path.read_text())
        data['sourceSha'] = 'b' * 40
        path.write_text(json.dumps(data))
        self.refresh_sums()
        self.call('verify', expected='failure', **{'source-sha': 'b' * 40})

    def test_checksum_file_tamper_is_rejected(self):
        self.call('stage')
        (self.out / 'SHA256SUMS').write_text('forged checksum list\n')
        self.call('verify', expected='failure')

    def test_omitted_console_is_rejected_even_with_signed_inventory(self):
        self.call('stage')
        path = self.out / 'inventory.json'
        data = json.loads(path.read_text())
        data['artifacts'].pop('console')
        path.write_text(json.dumps(data))
        self.resign_inventory()
        self.call('verify', expected='failure')

    def test_extra_or_symlink_output_is_rejected(self):
        self.call('stage')
        extra = self.out / 'unexpected.exe'
        extra.write_bytes(b'not an inventory member')
        self.call('verify', expected='failure')
        extra.unlink()
        target = self.out / NAMES['console']
        target.unlink()
        target.symlink_to(self.inputs['console'])
        self.call('verify', expected='failure')

    def test_invalid_metadata_refuses_before_signing(self):
        for changes in [{'source-sha': 'short'}, {'version': '../2.0.987'}, {'mirror': 'http://example.invalid'},
                        {'mirror': 'https://user:password@example.invalid'}, {'channel': 'nightly'}]:
            with self.subTest(changes=changes):
                self.call('stage', expected='failure', **changes)
                self.assertFalse(self.out.exists())

    def test_recorded_source_pins_are_the_helper_release_source_files(self):
        # The expected source pin set is the helper's own RELEASE_SOURCE_FILES plus the sibling
        # Cargo.lock, so the reader and the writer can never drift apart on the file list.
        self.call('stage')
        inventory = json.loads((self.out / 'inventory.json').read_text())
        self.assertEqual(set(inventory['tooling']['source']), EXPECTED_SOURCE_PIN_NAMES)
        helper = load_module(HELPER, 'revision8_release_helper')
        self.assertEqual(set(helper.RELEASE_SOURCE_FILES), set(EXPECTED_SOURCE_FILES))
        self.assertIn('src/document.rs', helper.RELEASE_SOURCE_FILES)
        for name in sorted(inventory['tooling']['source']):
            if name == 'Cargo.lock':
                path = self.root / name
            else:
                path = self.source_root / name
            self.assertEqual(inventory['tooling']['source'][name], hashlib.sha256(path.read_bytes()).hexdigest())

    def test_manifest_contract_is_accepted_by_the_writer_and_by_the_reader(self):
        # The manifest that the writer produces must be the one the reader accepts. The nine-key
        # contract is the helper's own: seven metadata keys plus the artifacts and the files.
        # This file must run beside the helper alone, so it resolves nothing outside HERE.
        self.call('stage')
        inventory = json.loads((self.out / 'inventory.json').read_text())
        expected_keys = {'schema', 'sourceSha', 'version', 'channel', 'mirror', 'minVersion',
                         'tooling', 'artifacts', 'files'}
        self.assertEqual(set(inventory), expected_keys)
        helper = load_module(HELPER, 'revision8_release_helper')
        arguments = types.SimpleNamespace(
            signer=SIGNER, out=self.out, version=VERSION, channel=CHANNEL, source_sha=SOURCE,
            mirror=MIRROR, launcher_pubkey=self.public['launcher'], release_pubkey=self.public['release'],
            min_version='0.1.0', release_source_root=self.source_root)
        # The writer's own metadata is the seven metadata keys; the stage adds artifacts and files.
        self.assertEqual(set(helper.metadata(arguments)),
                         {'schema', 'sourceSha', 'version', 'channel', 'mirror', 'minVersion', 'tooling'})
        self.assertTrue(set(helper.metadata(arguments)) < set(inventory))
        # The reader is the same helper: it accepts the manifest the writer just produced.
        ready = helper.verify(arguments)
        self.assertEqual(ready, hashlib.sha256((self.out / 'SHA256SUMS').read_bytes()).hexdigest() + '\n')
        self.assertTrue((self.out / 'READY').is_file())

    def test_named_signing_cli_pin_must_match_the_recorded_pair(self):
        # The named signing CLI is compared with the recorded pair, byte for byte. The reader here
        # is a different binary of the same sources than the one that signed, which is the handover
        # the named pin exists for: the reader's own bytes are not the recorded ones.
        self.call('stage')
        inventory = json.loads((self.out / 'inventory.json').read_text())
        recorded = inventory['tooling']['cli']
        reader = self.root / 'reader-summrise-release'
        reader.write_bytes(SIGNER.read_bytes() + b'\x00')
        reader.chmod(0o755)
        # The named pin is the recorded pair, so the reader accepts the bundle it did not sign.
        self.call('verify', signer=reader, **{'expected-cli-sha256': recorded['sha256'],
                                              'expected-cli-bytes': recorded['bytes']})
        # A named sha256 with the wrong byte length is refused: the pair stays a pair.
        result = self.call('verify', expected='failure', signer=reader,
                           **{'expected-cli-sha256': recorded['sha256'], 'expected-cli-bytes': 1})
        self.assertIn('recorded tooling pins differ', result.stderr)
        # A named sha256 that is not the recorded one is refused.
        result = self.call('verify', expected='failure', signer=reader,
                           **{'expected-cli-sha256': '00' * 32, 'expected-cli-bytes': recorded['bytes']})
        self.assertIn('recorded tooling pins differ', result.stderr)

    def test_half_a_named_signing_cli_pin_is_refused_by_name(self):
        # Half a pin is refused with a named reason, never silently dropped: a reader that names
        # only the hash, or only the byte length, must not fall back to a weaker comparison and
        # then report success.
        self.call('stage')
        inventory = json.loads((self.out / 'inventory.json').read_text())
        recorded = inventory['tooling']['cli']
        for changes in ({'expected-cli-sha256': recorded['sha256']},
                        {'expected-cli-bytes': recorded['bytes']}):
            with self.subTest(changes=sorted(changes)):
                result = self.call('verify', expected='failure', **changes)
                self.assertIn('the signing CLI pin must be given as a pair', result.stderr)
                self.assertFalse((self.out / 'READY').exists())
        # The writer records the CLI it uses, so a named pin belongs to the reader alone.
        result = self.call('stage', expected='failure',
                           **{'expected-cli-sha256': recorded['sha256'],
                              'expected-cli-bytes': recorded['bytes']})
        self.assertIn('belongs to the reader', result.stderr)

    def test_named_signing_cli_pin_never_relaxes_the_source_pin_comparison(self):
        # Naming the signing CLI changes only the CLI half of the comparison. The source pins are
        # always compared with the live bytes of the release source root.
        self.call('stage')
        inventory = json.loads((self.out / 'inventory.json').read_text())
        recorded = inventory['tooling']['cli']
        named = {'expected-cli-sha256': recorded['sha256'], 'expected-cli-bytes': recorded['bytes']}
        (self.source_root / 'src' / 'document.rs').write_text('// changed public test source bytes\n')
        result = self.call('verify', expected='failure', **named)
        self.assertIn('recorded tooling pins differ', result.stderr)
        self.assertFalse((self.out / 'READY').exists())

    def test_omitting_the_named_signing_cli_pin_keeps_the_stricter_comparison(self):
        # Without the named pin the recorded CLI bytes must be the bytes of the binary that
        # verifies the bundle, so omitting the option is never a way around the comparison.
        self.call('stage')
        self.call('verify')
        other = self.root / 'other-summrise-release'
        other.write_bytes(SIGNER.read_bytes() + b'\x00')
        other.chmod(0o755)
        result = self.call('verify', expected='failure', signer=other)
        self.assertIn('recorded tooling pins differ', result.stderr)
        self.assertFalse((self.out / 'READY').exists())

class Authority(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix=self._testMethodName + '-', dir=RUNTIME_ROOT))
        self.facts = {
            'main': {'sha': SOURCE}, 'tag-receipt': {'sha': SOURCE},
            'ci': {'head_sha': SOURCE, 'head_branch': 'main', 'event': 'push',
                   'path': '.github/workflows/ci.yml', 'status': 'completed', 'conclusion': 'success'},
            'installer': {'id': 271828, 'head_sha': SOURCE, 'head_branch': 'v' + VERSION,
                          'event': 'workflow_dispatch', 'path': '.github/workflows/installer.yml',
                          'status': 'in_progress', 'conclusion': None},
            'inventory': {'sourceSha': SOURCE, 'version': VERSION, 'channel': CHANNEL}}
        self.sequence = 0

    def call(self, expected=0, publication=False, **changes):
        argv = [sys.executable, str(HELPER), 'authority']
        for name, fact in self.facts.items():
            path = self.root / (name + '.json')
            path.write_text(json.dumps(fact))
            argv.extend(['--' + name, str(path)])
        opts = {'source-sha': SOURCE, 'checkout-sha': SOURCE, 'version': VERSION,
                'channel': CHANNEL, 'tag': 'v' + VERSION, 'installer-run-id': '271828'}
        opts.update(changes)
        for name, value in opts.items():
            argv.extend(['--' + name, value])
        if publication:
            argv.append('--require-installer-success')
        result = subprocess.run(argv, cwd=self.root, capture_output=True, text=True)
        label = str(self.sequence)
        self.sequence += 1
        (self.root / (label + '.exit')).write_text(str(result.returncode) + '\n')
        (self.root / (label + '.stderr.raw')).write_text(result.stderr)
        (self.root / (label + '.stdout.raw')).write_text(result.stdout)
        self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
        if expected:
            self.assertIn('release-bundle: authority', result.stderr)
        else:
            self.assertIn('AUTHORITY:', result.stdout)

    def test_all_source_authorities_must_agree_before_draft_upload(self):
        self.call()
        self.assertFalse((self.root / 'READY').exists())

    def test_each_changed_source_authority_refuses_upload(self):
        for name, field in [('main', 'sha'), ('tag-receipt', 'sha'), ('ci', 'head_sha'),
                            ('installer', 'head_sha'), ('inventory', 'sourceSha')]:
            with self.subTest(name=name):
                self.facts[name][field] = 'b' * 40
                self.call(expected=1)
                self.facts[name][field] = SOURCE
        self.call(expected=1, **{'checkout-sha': 'b' * 40})

    def test_ci_must_be_successful_push_on_main_for_exact_workflow(self):
        for field, wrong in [('head_branch', 'feature'), ('event', 'pull_request'), ('path', 'other.yml'),
                             ('status', 'in_progress'), ('conclusion', 'failure')]:
            with self.subTest(field=field):
                before = self.facts['ci'][field]
                self.facts['ci'][field] = wrong
                self.call(expected=1)
                self.facts['ci'][field] = before

    def test_installer_run_identity_and_tag_cannot_be_substituted(self):
        for field, wrong in [('id', 271829), ('path', 'other.yml'), ('event', 'pull_request'),
                             ('head_branch', 'main')]:
            with self.subTest(field=field):
                before = self.facts['installer'][field]
                self.facts['installer'][field] = wrong
                self.call(expected=1)
                self.facts['installer'][field] = before
        self.call(expected=1, tag='v2.0.988')

    def test_inventory_version_and_channel_are_authority_inputs(self):
        for field, wrong in [('version', '2.0.988'), ('channel', 'stable')]:
            with self.subTest(field=field):
                before = self.facts['inventory'][field]
                self.facts['inventory'][field] = wrong
                self.call(expected=1)
                self.facts['inventory'][field] = before

    def test_draft_asset_ids_names_sizes_and_exact_file_set_must_stay_stable(self):
        bundle = self.root / 'downloaded'
        bundle.mkdir()
        (bundle / 'artifact.exe').write_bytes(b'MZx')
        before = {'isDraft': True, 'tagName': 'v' + VERSION,
                  'assets': [{'id': 42, 'name': 'artifact.exe', 'size': 3}]}
        before_path, after_path = self.root / 'before.json', self.root / 'after.json'
        before_path.write_text(json.dumps(before))
        opts = {'release-before': str(before_path), 'release-after': str(after_path), 'bundle': str(bundle)}
        after_path.write_text(json.dumps(before))
        self.call(**opts)
        for field, wrong in [('id', 43), ('name', 'other.exe'), ('size', 4)]:
            after = json.loads(json.dumps(before))
            after['assets'][0][field] = wrong
            after_path.write_text(json.dumps(after))
            self.call(expected=1, **opts)
        for field, wrong in [('isDraft', False), ('tagName', 'v2.0.988')]:
            after = dict(before, **{field: wrong})
            after_path.write_text(json.dumps(after))
            self.call(expected=1, **opts)
        after = json.loads(json.dumps(before))
        after['assets'].append({'id': 43, 'name': 'extra.exe', 'size': 3})
        after_path.write_text(json.dumps(after))
        self.call(expected=1, **opts)

    def test_current_installer_run_must_not_be_failed_or_cancelled(self):
        self.facts['installer'].update(status='completed', conclusion='failure')
        self.call(expected=1)
        self.facts['installer'].update(status='completed', conclusion='cancelled')
        self.call(expected=1)
        self.facts['installer'].update(status='in_progress', conclusion=None)
        self.call()


if __name__ == '__main__':
    unittest.main(verbosity=2)
