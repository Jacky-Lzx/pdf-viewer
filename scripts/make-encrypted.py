#!/usr/bin/env python3
"""Build a deterministic legacy-encrypted PDF fixture, without external tools.

40-bit RC4 is used only to exercise PDF Standard Security revision 2 decoding.
It is not suitable for protecting documents.
"""
import hashlib
from pathlib import Path
import struct
import sys

PADDING = bytes.fromhex("28bf4e5e4e758a4164004e56fffa01082e2e00b6d0683e802f0ca9fe6453697a")


def md5(value):
    return hashlib.md5(value, usedforsecurity=False).digest()


def rc4(key, value):
    table = list(range(256))
    j = 0
    for i in range(256):
        j = (j + table[i] + key[i % len(key)]) % 256
        table[i], table[j] = table[j], table[i]
    i = j = 0
    output = bytearray()
    for byte in value:
        i = (i + 1) % 256
        j = (j + table[i]) % 256
        table[i], table[j] = table[j], table[i]
        output.append(byte ^ table[(table[i] + table[j]) % 256])
    return bytes(output)


def make_pdf(password="jkgdi"):
    padded = (password.encode("latin-1") + PADDING)[:32]
    owner = rc4(md5((b"pagewing-owner" + PADDING)[:32])[:5], padded)
    identifier = md5(b"Pagewing deterministic encrypted test document")
    permissions = -4
    key = md5(padded + owner + struct.pack("<i", permissions) + identifier)[:5]
    user = rc4(key, PADDING)
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Count 3 /Kids [4 0 R 6 0 R 8 0 R] >>",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ]
    for page in range(3):
        stream_id = len(objects) + 2
        objects.append(f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Resources << /Font << /F1 3 0 R >> >> /Contents {stream_id} 0 R >>".encode())
        content = f"BT /F1 18 Tf 50 700 Td (Encrypted document page {page + 1}.) Tj 0 -30 Td (Rust password verification.) Tj ET".encode()
        object_key = md5(key + struct.pack("<I", stream_id)[:3] + b"\0\0")[:10]
        encrypted = rc4(object_key, content)
        objects.append(f"<< /Length {len(encrypted)} >>\nstream\n".encode() + encrypted + b"\nendstream")
    encryption_id = len(objects) + 1
    objects.append(f"<< /Filter /Standard /V 1 /R 2 /Length 40 /O <{owner.hex()}> /U <{user.hex()}> /P {permissions} >>".encode())
    output = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
    offsets = [0]
    for number, value in enumerate(objects, 1):
        offsets.append(len(output))
        output.extend(f"{number} 0 obj\n".encode() + value + b"\nendobj\n")
    xref = len(output)
    output.extend(f"xref\n0 {len(offsets)}\n0000000000 65535 f \n".encode())
    for offset in offsets[1:]:
        output.extend(f"{offset:010} 00000 n \n".encode())
    output.extend(f"trailer\n<< /Size {len(offsets)} /Root 1 0 R /Encrypt {encryption_id} 0 R /ID [<{identifier.hex()}> <{identifier.hex()}>] >>\nstartxref\n{xref}\n%%EOF\n".encode())
    return bytes(output)


if __name__ == "__main__":
    path = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("/tmp/pagewing-encrypted.pdf")
    path.write_bytes(make_pdf())
    print(path)
