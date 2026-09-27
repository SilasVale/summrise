#!/usr/bin/env python3
"""pe-icon-check.py <exe> — THE EXE MUST CARRY AN ICON RESOURCE, AND NOTHING READ THE TABLE.

WHY (round 67 of the standing goal). `agent/build.rs` records the defect: `summrise-agent.exe` once shipped with NO
resource of any kind, so Task Manager drew the generic process glyph for the product's own agent. It also records the
blind spot — "no test reads a PE resource table" — and rounds 65-66 established why: **no CI job ever has an exe** (the
pack-chain job is named "npm artifact gates, no exe"), so this check can only live where an artifact exists.

`build.rs` already panics when `brand/icon.ico` is MISSING, which is the easy half. This catches the other half: the file
present and the resource compiler silently failing to embed it. Its negative case must be built by hand (a copy with the
resource directory stripped) — a rebuild cannot produce one, because the build refuses first.

Exit 0 when an RT_ICON (type 3) resource is present; exit 1 with a reason otherwise.

WHY IT LIVES IN `scripts/` AND NOT `scripts/test/` (round 67, measured): `scripts/test/build-pins.bash` FAILS with
"every scripts/test file is invoked from ci.yml", and this one CANNOT be — no CI job has an exe to read (round 65). Putting
it there turned `main` red in two jobs, which is the repository's own rule saying where a check that needs an artifact
belongs: beside the release flow that produces one.

MUTATION: zero the resource data directory entry (index 2) in a COPY of the exe —
    python3 -c "import struct;b=bytearray(open(SRC,'rb').read());pe=struct.unpack_from('<I',b,0x3C)[0];opt=pe+24;dd=opt+112;struct.pack_into('<II',b,dd+16,0,0);open(DST,'wb').write(b)"
  (PE32+ here; for PE32 the fixed optional-header size is 96 rather than 112.)
RESULT:   exit 1 — "the resource data directory is EMPTY — this exe carries no resources at all".
  AND THE MUTATION MUST BE BUILT BY HAND, WHICH ROUND 66 ESTABLISHED: renaming `brand/icon.ico` away and rebuilding
  produces NO EXE AT ALL, because `agent/build.rs` panics first. A proof written as a rebuild would pass vacuously.
POSITIVE: exit 0 on the real artifact — "carries RT_ICON; resource types present: [3, 14, 16]".
"""
import struct, sys

RT_ICON, RT_GROUP_ICON = 3, 14


def die(msg):
    print(f"pe-icon-check: FAIL — {msg}")
    sys.exit(1)


def main(path):
    with open(path, "rb") as fh:
        b = fh.read()
    if b[:2] != b"MZ":
        die(f"{path} is not a PE file (no MZ header)")
    pe = struct.unpack_from("<I", b, 0x3C)[0]
    if b[pe:pe + 4] != b"PE\0\0":
        die(f"{path} has no PE signature at 0x{pe:x}")
    coff = pe + 4
    nsec, = struct.unpack_from("<H", b, coff + 2)
    optsize, = struct.unpack_from("<H", b, coff + 16)
    opt = coff + 20
    magic, = struct.unpack_from("<H", b, opt)
    # Data directories start after the fixed part: 96 bytes for PE32, 112 for PE32+.
    dd = opt + (96 if magic == 0x10B else 112)
    rsrc_rva, rsrc_size = struct.unpack_from("<II", b, dd + 2 * 8)
    if rsrc_rva == 0 or rsrc_size == 0:
        die("the resource data directory is EMPTY — this exe carries no resources at all")
    # RVA -> file offset, through the section table.
    secs = []
    for i in range(nsec):
        s = opt + optsize + i * 40
        va, vsize = struct.unpack_from("<II", b, s + 12)
        raw, rawsize = struct.unpack_from("<II", b, s + 20)
        secs.append((va, vsize, raw, rawsize))
    def off(rva):
        for va, vsize, raw, rawsize in secs:
            if va <= rva < va + max(vsize, rawsize):
                return raw + (rva - va)
        die(f"rva 0x{rva:x} is in no section")
    # The type table: entries of (id, offset-to-directory), high bit = name string.
    base = off(rsrc_rva)
    nname, nid = struct.unpack_from("<HH", b, base + 12)
    types = []
    for i in range(nname + nid):
        e = base + 16 + i * 8
        ident, sub = struct.unpack_from("<II", b, e)
        types.append(ident & 0x7FFFFFFF if not (ident & 0x80000000) else -1)
    if RT_ICON not in types:
        die(f"no RT_ICON (type {RT_ICON}) in the resource type table — found {sorted(t for t in types if t > 0)}")
    print(f"pe-icon-check: OK — {path} carries RT_ICON; resource types present: {sorted(t for t in types if t > 0)}"
          + ("" if RT_GROUP_ICON in types else " (NOTE: no RT_GROUP_ICON — Windows may still draw nothing)"))
    sys.exit(0)


if __name__ == "__main__":
    if len(sys.argv) != 2:
        print("usage: pe-icon-check.py <exe>", file=sys.stderr)
        sys.exit(2)
    main(sys.argv[1])
