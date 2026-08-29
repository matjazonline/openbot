#!/usr/bin/env bash
#
# Report the stack frame each function reserves, largest first.
#
# Why this exists: an `async fn` poll frame materialises the child future it constructs, so in an
# unoptimized build a one-line wrapper can cost hundreds of KiB and a deep `await` chain can walk
# off the end of a thread stack. That is what aborted the task worker on 2026-08-29 -- 1,997 KiB of
# a 2,080 KiB Tokio worker stack, spent before `AgentBuilder::from_yaml` was even called. Run this
# after touching the task-worker or dispatch chain and compare against the last recorded numbers.
#
# Usage:
#   scripts/stack-frames.sh [binary] [threshold-kib]
#
# Defaults to target/debug/mail_agents and 32 KiB. Pass a lower threshold to see the tail.
#
# arm64/Mach-O only -- it decodes the prologue directly, so there is no extra tooling to install.

set -euo pipefail

BINARY="${1:-target/debug/mail_agents}"
THRESHOLD_KIB="${2:-32}"

if [[ ! -f "$BINARY" ]]; then
    echo "no such binary: $BINARY" >&2
    echo "build it first, e.g. 'cargo build' or 'cargo build --release'" >&2
    exit 1
fi

if [[ "$(uname -m)" != "arm64" ]]; then
    echo "this script decodes arm64 prologues; $(uname -m) is not supported" >&2
    exit 1
fi

nm -n "$BINARY" | python3 -c '
import struct, sys

BINARY = sys.argv[1]
THRESHOLD = int(sys.argv[2]) * 1024
# __TEXT is linked at this address for a Mach-O executable, so file offset == vmaddr - BASE.
BASE = 0x100000000

syms = []
for line in sys.stdin:
    parts = line.split(None, 2)
    if len(parts) == 3 and parts[1] in ("T", "t"):
        syms.append((int(parts[0], 16), parts[2].strip()))
syms.sort()

data = open(BINARY, "rb").read()


def sub_sp(word):
    """Decode `sub Xd, sp, #imm{, lsl #12}`; returns (Rd, imm) or None."""
    if (word >> 23) & 0x1FF != 0b110100010:
        return None
    if (word >> 5) & 0x1F != 31:          # Rn must be sp
        return None
    imm = (word >> 10) & 0xFFF
    if (word >> 22) & 1:                  # the lsl #12 bit
        imm <<= 12
    return (word & 0x1F, imm)


def frame_size(addr, end):
    """Bytes of stack the prologue at `addr` claims.

    Three forms appear. `stp x29, x30, [sp, #-N]!` pre-indexes the frame record. `sub sp, sp, #N`
    claims the rest. A frame past a page uses a probe loop -- `sub x9, sp, #N, lsl #12` computes
    the target, then `sub sp, sp, #0x1000` / `str xzr, [sp]` / `cmp sp, x9` / `b.ne` walks down to
    it touching each page. A scan that misses the third form undercounts the big frames by ~20x,
    which is the only form that matters here.
    """
    count = min(32, (end - addr) // 4)
    if count <= 0:
        return 0
    words = [struct.unpack_from("<I", data, addr - BASE + i * 4)[0] for i in range(count)]

    total = 0
    probe = None
    loop_end = None
    for i, word in enumerate(words):
        if (word & 0xFFC003E0) == 0xA98003E0:      # stp Xt, Xt2, [sp, #-imm7]!
            imm7 = (word >> 15) & 0x7F
            if imm7 & 0x40:
                imm7 -= 0x80
            total += -imm7 * 8
        if (word & 0xFF000010) == 0x54000000 and (word & 0xF) == 1:   # b.ne, ending the probe loop
            loop_end = i
        decoded = sub_sp(word)
        if decoded and decoded[0] != 31 and probe is None:
            probe = decoded[1]

    if probe is None:
        for word in words:
            decoded = sub_sp(word)
            if decoded and decoded[0] == 31:
                total += decoded[1]
        return total

    # A probe loop claims `probe` bytes; only the sub after the loop adds to it.
    total += probe
    for i, word in enumerate(words):
        if loop_end is not None and i <= loop_end:
            continue
        decoded = sub_sp(word)
        if decoded and decoded[0] == 31:
            total += decoded[1]
    return total


ESCAPES = {
    "$u7b$": "{", "$u7d$": "}", "$LT$": "<", "$GT$": ">", "$u20$": " ",
    "$C$": ",", "$RF$": "&", "$LP$": "(", "$RP$": ")", "$BP$": "*",
    "$u5b$": "[", "$u5d$": "]", "$u27$": chr(39), "..": "::",
}


def demangle(symbol):
    """Enough of the legacy Rust scheme to read the output; falls back to the raw symbol."""
    name = symbol[1:] if symbol.startswith("__ZN") else symbol
    if not name.startswith("_ZN") or not name.endswith("E"):
        return symbol
    body, i, parts = name[3:-1], 0, []
    while i < len(body):
        j = i
        while j < len(body) and body[j].isdigit():
            j += 1
        if j == i:
            break
        length = int(body[i:j])
        parts.append(body[j:j + length])
        i = j + length
    if parts and parts[-1].startswith("h") and len(parts[-1]) == 17:
        parts.pop()                       # the codegen hash carries no meaning here
    out = "::".join(parts)
    for escaped, plain in ESCAPES.items():
        out = out.replace(escaped, plain)
    return out


rows = []
for i, (addr, name) in enumerate(syms):
    if "mail_agents" not in name:
        continue
    end = syms[i + 1][0] if i + 1 < len(syms) else addr + 0x1000
    size = frame_size(addr, end)
    if size >= THRESHOLD:
        rows.append((size, demangle(name)))

rows.sort(reverse=True)
print(f"{len(rows)} functions at or above {THRESHOLD // 1024} KiB in {BINARY}\n")
for size, name in rows:
    print(f"{size // 1024:6} KiB  {name}")
if rows:
    print(f"\nlargest single frame: {rows[0][0] // 1024} KiB")
' "$BINARY" "$THRESHOLD_KIB"
