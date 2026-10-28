#!/usr/bin/env python3
"""Serif text, transparency, text modes, vectors and pixels for weight checks."""
from pathlib import Path


def make_pdf():
    content = b"""BT /F1 24 Tf 20 205 Td (Times: Reading Oeg8) Tj ET
q /GS1 gs BT /F1 24 Tf 20 160 Td (O   e   g   8) Tj ET Q
q 0.3 w BT /F1 24 Tf 1 Tr 20 115 Td (Stroke Oeg8) Tj ET Q
q BT /F1 24 Tf 3 Tr 20 80 Td (Invisible) Tj ET Q
q BT /F1 24 Tf 7 Tr 200 80 Td (Clip) Tj ET 1 0 0 rg 0 0 400 240 re f Q
q 0.1 0.4 0.8 rg 20 15 100 35 re f Q
q 0.5 w 140 25 m 380 25 l S Q
q 30 0 0 30 140 40 cm /Im0 Do Q
"""
    pixels = bytes([255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0])
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 240] /Resources << /Font << /F1 4 0 R >> /ExtGState << /GS1 5 0 R >> /XObject << /Im0 6 0 R >> >> /Contents 7 0 R >>",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman >>",
        b"<< /Type /ExtGState /ca 0.5 /CA 0.5 >>",
        b"<< /Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Length 12 >>\nstream\n" + pixels + b"\nendstream",
        f"<< /Length {len(content)} >>\nstream\n".encode() + content + b"endstream",
    ]
    output = bytearray(b"%PDF-1.7\n")
    offsets = [0]
    for number, item in enumerate(objects, 1):
        offsets.append(len(output))
        output.extend(f"{number} 0 obj\n".encode() + item + b"\nendobj\n")
    xref = len(output)
    output.extend(f"xref\n0 {len(offsets)}\n0000000000 65535 f \n".encode())
    for offset in offsets[1:]:
        output.extend(f"{offset:010} 00000 n \n".encode())
    output.extend(f"trailer\n<< /Size {len(offsets)} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode())
    return output


if __name__ == "__main__":
    (Path(__file__).resolve().parent.parent / "tests/fixtures/text-weight.pdf").write_bytes(make_pdf())
