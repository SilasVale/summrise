#!/usr/bin/env python3
"""Stage already-built release bytes; never build, fetch, or publish anything."""
import argparse
import hashlib
import json
import re
import shutil
import subprocess
import sys
from pathlib import Path
from urllib.parse import urlsplit

ROLES = ('launch', 'device', 'console', 'shell', 'setup')
INVENTORY, INVENTORY_SIG, SUMS, READY = 'inventory.json', 'inventory.sidecar.json', 'SHA256SUMS', 'READY'
# The release sources that decide what the verifier accepts. The bundle records their bytes so a
# reader can say which source built the command that signed it. `src/document.rs` is the module
# that implements document::verify_at, the policy freshness check and the two inner signatures,
# so a drift there must be visible to the comparison rather than only through the CLI bytes.
RELEASE_SOURCE_FILES = ('src/verify.rs', 'src/lib.rs', 'src/document.rs',
                        'src/bin/summrise-release.rs', 'tests/cli.rs')


def require(condition, message):
    if not condition:
        raise ValueError(message)


def record(path):
    digest, size = hashlib.sha256(), 0
    require(path.is_file() and not path.is_symlink(), f'not a regular file: {path.name}')
    with path.open('rb') as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b''):
            digest.update(block)
            size += len(block)
    return {'sha256': digest.hexdigest(), 'bytes': size}


def run(a, command, *args):
    result = subprocess.run([str(a.signer), command, *map(str, args)],
                            capture_output=True, text=True, timeout=120)
    require(result.returncode == 0, f'{command} failed (exit {result.returncode}): {result.stderr.strip()}')
    return result.stdout.strip()


def cli_pin(path):
    require(path is not None, 'the trusted CLI is missing')
    require(path.is_file(), f'the trusted CLI is not a regular file: {path.name}')
    digest, size = hashlib.sha256(), 0
    with path.open('rb') as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b''):
            digest.update(block)
            size += len(block)
    return {'sha256': digest.hexdigest(), 'bytes': size}


def source_pins(root):
    require(root is not None, 'the release source root is missing')
    pins = {}
    for name in RELEASE_SOURCE_FILES:
        path = root / name
        require(path.is_file() and not path.is_symlink(), f'release source pin is missing: {name}')
        pins[name] = record(path)['sha256']
    lock = root.parent / 'Cargo.lock'
    require(lock.is_file() and not lock.is_symlink(), 'release source pin is missing: Cargo.lock')
    pins['Cargo.lock'] = record(lock)['sha256']
    return pins


def tooling(a):
    return {'cli': cli_pin(a.signer), 'source': source_pins(a.release_source_root)}


def expected_tooling(a):
    """The tooling section a reader must find in the manifest.

    The source pins are always compared with the live bytes of the release source root. The CLI
    bytes are compared with the binary that verifies the bundle, unless the reader names the
    signing CLI explicitly: the release is signed by the CI build and read by the delivery host,
    which is a different binary of the same sources. Naming the signing CLI keeps the pair
    (CLI bytes, source pins) intact across that handover instead of pretending one binary does
    both jobs.

    The named CLI pin is a pair, sha256 and byte length together. Half a pair is refused by name
    rather than silently dropped: a reader that names only the hash, or only the length, must
    not fall back to a weaker comparison and then report success.
    """
    sha = getattr(a, 'expected_cli_sha256', None)
    size = getattr(a, 'expected_cli_bytes', None)
    require((sha is None) == (size is None),
            'the signing CLI pin must be given as a pair: --expected-cli-sha256 and '
            '--expected-cli-bytes are both required, or both omitted')
    expected = tooling(a)
    if sha is not None:
        require(re.fullmatch(r'[0-9a-f]{64}', sha) is not None,
                'the named signing CLI sha256 is not a lowercase 64-hex digest')
        require(type(size) is int and size > 0,
                'the named signing CLI byte length is not a positive integer')
        expected['cli'] = {'sha256': sha, 'bytes': size}
    return expected


def require_cli_pin_pair(a, role):
    """Refuse a half-given CLI pin by name, before any byte is written or read."""
    sha = getattr(a, 'expected_cli_sha256', None)
    size = getattr(a, 'expected_cli_bytes', None)
    require((sha is None) == (size is None),
            'the signing CLI pin must be given as a pair: --expected-cli-sha256 and '
            '--expected-cli-bytes are both required, or both omitted')
    require(sha is None or role == 'reader',
            'a named signing CLI pin belongs to the reader; the writer records the CLI it uses')
    if sha is not None:
        require(re.fullmatch(r'[0-9a-f]{64}', sha) is not None,
                'the named signing CLI sha256 is not a lowercase 64-hex digest')
        require(type(size) is int and size > 0,
                'the named signing CLI byte length is not a positive integer')


def sidecar(name):
    return str(Path(name).with_suffix('.sidecar.json'))


def layout(a):
    artifacts = {role: (f'SummriseSetup-{a.version}-{a.channel}.exe' if role == 'setup' else
                       f'summrise-{role}-{a.version}-{a.channel}.exe') for role in ROLES}
    doc = f'release-{a.version}-{a.channel}.json'
    files = set(artifacts.values()) | {sidecar(n) for n in artifacts.values()} | {doc, sidecar(doc)}
    return artifacts, doc, files


def metadata(a):
    require(re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)', a.version), 'invalid version')
    require(re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', a.min_version), 'invalid minimum version')
    require(a.channel in ('stable', 'canary'), 'invalid channel')
    require(re.fullmatch(r'[0-9a-f]{40}', a.source_sha), 'source SHA must be exact lowercase 40-hex commit')
    for key in (a.launcher_pubkey, a.release_pubkey):
        require(re.fullmatch(r'[0-9a-f]{64}', key), 'invalid public key')
    mirror = urlsplit(a.mirror)
    require(mirror.scheme == 'https' and mirror.hostname and not mirror.username and not mirror.password
            and not mirror.query and not mirror.fragment and not any(c.isspace() for c in a.mirror)
            and not a.mirror.endswith('/'), 'mirror must be an HTTPS base without credentials/query/fragment/trailing slash')
    require(not a.out.is_symlink(), 'output directory must not be a symlink')
    return {'schema': 1, 'sourceSha': a.source_sha, 'version': a.version, 'channel': a.channel,
            'mirror': a.mirror, 'minVersion': a.min_version, 'tooling': tooling(a)}


def write_new(path, text):
    with path.open('x', encoding='utf-8', newline='\n') as handle:
        handle.write(text)


def sign_sidecar(a, artifact, seed):
    run(a, 'sidecar', '--key-file', seed, '--artifact', a.out / artifact,
        '--version', a.version, '--channel', a.channel, '--out', a.out / sidecar(artifact))


def verify_artifact(a, artifact, public):
    run(a, 'verify-artifact', '--artifact', a.out / artifact, '--sidecar', a.out / sidecar(artifact),
        '--public-key', public, '--version', a.version, '--channel', a.channel)


def sums(a, files):
    return ''.join(f"{record(a.out / name)['sha256']}  {name}\n" for name in sorted(files))


def verify(a):
    require_cli_pin_pair(a, 'reader')
    expected = metadata(a)
    artifacts, doc, files = layout(a)
    require(a.out.is_dir(), 'bundle directory is missing')
    immutable = files | {INVENTORY, INVENTORY_SIG}
    actual = {path.name for path in a.out.iterdir()}
    require(actual in (immutable | {SUMS}, immutable | {SUMS, READY}), 'bundle file set differs')
    for name in actual:
        record(a.out / name)
    inventory = json.loads((a.out / INVENTORY).read_text(encoding='utf-8'))
    require(isinstance(inventory, dict) and set(inventory) == set(expected) | {'artifacts', 'files'},
            'inventory schema differs')
    # The tooling comparison runs before the first call to the signer. The CLI half needs no
    # signer capability: it compares the recorded CLI bytes with the reader's own bytes, or with
    # the signing CLI the reader names. A signer that is not the CLI this bundle records is
    # therefore refused by name, and the refusal does not depend on what that binary can do.
    # Every check here is a refusal: the bundle is still accepted only after the inventory's own
    # signature and every artifact and document signature have been verified below.
    require(inventory.get('tooling') == expected_tooling(a),
            'recorded tooling pins differ: the CLI bytes or the release source pins are not the ones this bundle names')
    verify_artifact(a, INVENTORY, a.release_pubkey)
    require(all(inventory[k] == value for k, value in expected.items() if k != 'tooling'),
            'inventory source/version/channel/mirror differs')
    require(inventory['artifacts'] == artifacts and set(inventory['files']) == files, 'inventory is incomplete')
    for name in files:
        require(inventory['files'][name] == record(a.out / name), f'changed bytes or length: {name}')
    for name in artifacts.values():
        verify_artifact(a, name, a.launcher_pubkey)
    verify_artifact(a, doc, a.release_pubkey)
    run(a, 'verify-document', '--document', a.out / doc, '--public-key', a.release_pubkey,
        '--version', a.version, '--channel', a.channel)
    document = json.loads((a.out / doc).read_text(encoding='utf-8'))
    policy = {'version': a.version, 'channel': a.channel, 'sha256': record(a.out / artifacts['device'])['sha256'],
              'artifactUrl': a.mirror + '/' + artifacts['device'], 'minVersion': a.min_version}
    require(all(document[k] == value for k, value in policy.items()), 'update document policy differs')
    require(set(document) == set(policy) | {'issuedAtMs', 'artifactSignature', 'signature'}, 'update document schema differs')
    require(type(document['issuedAtMs']) is int and document['issuedAtMs'] > 0, 'invalid document issue time')
    require(all(re.fullmatch(r'[0-9a-f]{128}', document[k]) for k in ('artifactSignature', 'signature')),
            'invalid update signature shape')
    require((a.out / SUMS).read_text(encoding='utf-8') == sums(a, immutable), 'checksum closure differs')
    ready_text = record(a.out / SUMS)['sha256'] + '\n'
    if (a.out / READY).exists():
        require((a.out / READY).read_text(encoding='utf-8') == ready_text, 'READY checksum pin differs')
    return ready_text


def stage(a):
    require_cli_pin_pair(a, 'writer')
    facts = metadata(a)
    require(not a.out.exists() or (a.out.is_dir() and not any(a.out.iterdir())), 'output collision; refusing overwrite')
    sources = [getattr(a, role) for role in ROLES]
    require(len(set(path.resolve() for path in sources)) == len(ROLES), 'artifact input collision')
    for path in sources:
        require(record(path)['bytes'] > 0, f'empty artifact: {path.name}')
        require(a.out.resolve() not in path.resolve().parents, 'input is inside destination')
    for role in ('launcher', 'release'):
        seed = getattr(a, role + '_seed')
        require(a.out.resolve() not in seed.resolve().parents, 'seed is inside destination')
        require(run(a, 'pubkey', '--key-file', seed) == getattr(a, role + '_pubkey'), 'seed/public key mismatch')
    if not a.out.exists():
        a.out.mkdir(mode=0o700)
    artifacts, doc, files = layout(a)
    for role, name in artifacts.items():
        with getattr(a, role).open('rb') as src, (a.out / name).open('xb') as dst:
            shutil.copyfileobj(src, dst)
        sign_sidecar(a, name, a.launcher_seed)
    run(a, 'sign', '--key-file', a.release_seed, '--artifact', a.out / artifacts['device'],
        '--version', a.version, '--channel', a.channel, '--url', a.mirror + '/' + artifacts['device'],
        '--min-version', a.min_version, '--out', a.out / doc)
    sign_sidecar(a, doc, a.release_seed)
    facts.update(artifacts=artifacts, files={name: record(a.out / name) for name in sorted(files)})
    write_new(a.out / INVENTORY, json.dumps(facts, sort_keys=True, indent=2) + '\n')
    sign_sidecar(a, INVENTORY, a.release_seed)
    write_new(a.out / SUMS, sums(a, files | {INVENTORY, INVENTORY_SIG}))
    write_new(a.out / READY, verify(a))


def invalidate_ready(out):
    # A failed check removes only this owned directory's fixed marker, preserving all release bytes.
    ready = out / READY
    expected = out.resolve() / READY
    if out.is_dir() and not out.is_symlink() and ready.is_file() and not ready.is_symlink():
        require(ready.resolve() == expected, 'READY resolved outside destination')
        ready.unlink()


def authority(a):
    facts = [json.loads(getattr(a, name).read_text(encoding='utf-8'))
             for name in ('main', 'tag_receipt', 'ci', 'installer', 'inventory')]
    require(all(isinstance(f, dict) for f in facts), 'authority receipt is not an object')
    main, tag, ci, installer, inventory = facts
    require(re.fullmatch(r'[0-9a-f]{40}', a.source_sha) and a.checkout_sha == a.source_sha,
            'authority checkout/source SHA differs')
    require(main.get('sha') == tag.get('sha') == ci.get('head_sha') == installer.get('head_sha')
            == inventory.get('sourceSha') == a.source_sha, 'authority source SHA differs')
    require(a.tag == 'v' + a.version and inventory.get('version') == a.version
            and inventory.get('channel') == a.channel, 'authority tag/version/channel differs')
    require(all(ci.get(k) == v for k, v in {'head_branch': 'main', 'event': 'push',
            'path': '.github/workflows/ci.yml', 'status': 'completed', 'conclusion': 'success'}.items()),
            'authority exact main CI is not successful')
    require(str(installer.get('id')) == a.installer_run_id and installer.get('head_branch') == a.tag
            and installer.get('path') == '.github/workflows/installer.yml'
            and installer.get('event') in ('push', 'workflow_dispatch'), 'authority installer identity differs')
    require((installer.get('status'), installer.get('conclusion')) in
            (('in_progress', None), ('completed', 'success')), 'authority installer run failed or is not running')
    receipts = (a.release_before, a.release_after, a.bundle)
    require(not any(receipts) or all(receipts), 'authority asset receipt arguments are incomplete')
    if all(receipts):
        snapshots = [json.loads(path.read_text(encoding='utf-8')) for path in receipts[:2]]
        local = {p.name: record(p)['bytes'] for p in a.bundle.iterdir()}
        identities = []
        for snapshot in snapshots:
            require(snapshot.get('isDraft') is True and snapshot.get('tagName') == a.tag,
                    'authority release is not the expected draft')
            assets = snapshot.get('assets', [])
            found = {p['name']: (p['id'], p['size']) for p in assets}
            require(len(found) == len(assets) and len({p['id'] for p in assets}) == len(assets)
                    and {name: value[1] for name, value in found.items()} == local
                    and all(size > 0 for size in local.values()), 'authority draft file set or sizes differ')
            identities.append(found)
        require(identities[0] == identities[1], 'authority draft asset identities changed')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    for mode in ('stage', 'verify'):
        p = commands.add_parser(mode)
        for name in ('signer', 'out'):
            p.add_argument('--' + name, type=Path, required=True)
        for name in ('version', 'channel', 'source-sha', 'mirror', 'launcher-pubkey', 'release-pubkey'):
            p.add_argument('--' + name, required=True)
        p.add_argument('--min-version', default='0.1.0')
        p.add_argument('--release-source-root', type=Path, required=True,
                       help='the device/release sources that built the trusted CLI')
        p.add_argument('--expected-cli-sha256',
                       help='the sha256 of the CLI that signed the bundle, when the reader is a '
                            'different binary of the same sources than the one that signed')
        p.add_argument('--expected-cli-bytes', type=int,
                       help='the byte length of the CLI that signed the bundle')
        if mode == 'stage':
            for name in (*ROLES, 'launcher-seed', 'release-seed'):
                p.add_argument('--' + name, type=Path, required=True)
    p = commands.add_parser('authority', help='validate fresh CI/API receipts after bundle verification')
    for name in ('main', 'tag-receipt', 'ci', 'installer', 'inventory'):
        p.add_argument('--' + name, type=Path, required=True)
    for name in ('source-sha', 'checkout-sha', 'tag', 'version', 'channel', 'installer-run-id'):
        p.add_argument('--' + name, required=True)
    for name in ('release-before', 'release-after', 'bundle'):
        p.add_argument('--' + name, type=Path)
    a = parser.parse_args()
    try:
        if a.command == 'authority':
            authority(a)
            print('AUTHORITY: checkout/main/tag/CI/installer/inventory agree for ' + a.source_sha)
            return 0
        if a.command == 'stage':
            stage(a)
        else:
            ready_text = verify(a)
            if not (a.out / READY).exists():
                write_new(a.out / READY, ready_text)
        print('READY: complete signed bundle verified for source ' + a.source_sha)
        return 0
    except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError) as error:
        if a.command == 'verify':
            invalidate_ready(a.out)
        print('release-bundle: ' + str(error), file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())
