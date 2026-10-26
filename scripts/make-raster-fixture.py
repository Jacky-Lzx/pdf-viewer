#!/usr/bin/env python3
"""Deterministic cropped, rotated PDF with transparency, vectors, text and RGB pixels."""
from pathlib import Path


def make_pdf():
    objects = [b"<< /Type /Catalog /Pages 2 0 R >>", b"",
               b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
               b"<< /Type /ExtGState /ca 0.5 /CA 0.5 >>"]
    pixels = bytes([255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0])
    objects.append(b"<< /Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Length 12 >>\nstream\n" + pixels + b"\nendstream")
    content = b"q /GS1 gs 0.1 0.4 0.8 rg 0 0 120 80 re f Q\nq 1 0.2 0.1 rg 20 20 30 40 re f Q\nBT /F1 9 Tf 12 62 Td (Cropped image) Tj ET\nq 60 0 0 20 25 25 cm /Im0 Do Q"
    objects.append(f"<< /Length {len(content)} >>\nstream\n".encode() + content + b"\nendstream")
    for rotation in [0, 90, 180, 270]:
        objects.append(f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 80] /CropBox [10 15 110 75] /Rotate {rotation} /UserUnit 2 /Resources << /Font << /F1 3 0 R >> /ExtGState << /GS1 4 0 R >> /XObject << /Im0 5 0 R >> >> /Contents 6 0 R >>".encode())
    objects[1] = b"<< /Type /Pages /Count 4 /Kids [7 0 R 8 0 R 9 0 R 10 0 R] >>"
    output = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
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
    (Path(__file__).resolve().parent.parent / "tests/fixtures/render-crop.pdf").write_bytes(make_pdf())
