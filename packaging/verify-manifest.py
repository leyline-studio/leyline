#!/usr/bin/env python3
"""Check `target/release-manifest/latest.json` the way the updater will.

Each signature in the manifest is verified over the local artifact it names,
against the public key compiled into Studio (`PUBLIC_KEY` in
`crates/leyline-studio/src/updates.rs`) — not against the `.pub` file next
to the private key, which could differ from what the binary ships. A copy of
each artifact with one byte flipped must then be refused, so a verifier that
accepts everything cannot pass for one that works (ADR 0077).

    python3 packaging/verify-manifest.py

The format is minisign: `ED` signatures cover the BLAKE2b-512 of the file,
and a second signature covers the first one plus its trusted comment.
"""

import base64
import hashlib
import json
import re
import sys
from pathlib import Path

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

ROOT = Path(__file__).resolve().parent.parent

# Where each platform's artifact is built, by manifest key.
LOCAL_DIRS = {
    "linux-x86_64": ROOT / "target/release",
    "windows-x86_64": ROOT / "target/x86_64-pc-windows-gnu/release",
    "darwin-x86_64": ROOT / "target/release",
}


def compiled_key():
    source = (ROOT / "crates/leyline-studio/src/updates.rs").read_text()
    encoded = re.search(r'const PUBLIC_KEY[^"]*"([A-Za-z0-9+/=]+)"', source, re.S)
    decoded = base64.b64decode(base64.b64decode(encoded.group(1)).decode().splitlines()[1])
    if decoded[:2] != b"Ed":
        sys.exit("PUBLIC_KEY is not a minisign Ed25519 key")
    return decoded[2:10], Ed25519PublicKey.from_public_bytes(decoded[10:42])


def verify(key_id, key, data, signature):
    lines = base64.b64decode(signature).decode().splitlines()
    raw = base64.b64decode(lines[1])
    algorithm, signer_id, value = raw[:2], raw[2:10], raw[10:74]
    if signer_id != key_id:
        raise InvalidSignature("signed by another key than the compiled one")
    key.verify(value, hashlib.blake2b(data).digest() if algorithm == b"ED" else data)
    trusted = lines[2].split("trusted comment: ", 1)[1].encode()
    key.verify(base64.b64decode(lines[3]), value + trusted)


def main():
    key_id, key = compiled_key()
    manifest = json.loads((ROOT / "target/release-manifest/latest.json").read_text())
    failed = False
    for platform, entry in manifest["platforms"].items():
        name = entry["url"].rsplit("/", 1)[1]
        data = bytearray((LOCAL_DIRS[platform] / name).read_bytes())
        try:
            verify(key_id, key, bytes(data), entry["signature"])
            print(f"  ok  {platform}: {name}")
        except InvalidSignature as error:
            failed = True
            print(f"  FAILED  {platform}: {name} — {error or 'bad signature'}")
        data[len(data) // 2] ^= 1
        try:
            verify(key_id, key, bytes(data), entry["signature"])
            failed = True
            print(f"  FAILED  {platform}: a copy with one byte flipped still verifies")
        except InvalidSignature:
            pass
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
