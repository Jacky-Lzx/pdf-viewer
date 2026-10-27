#!/usr/bin/env python3
"""Generate a PDF whose single glyph maps to a long Unicode text layer."""
from pathlib import Path

TEXT = "Rust cafe\u0301 中文 👩‍💻 word " * 200


def make_pdf():
    mapping = TEXT.encode("utf-16-be").hex()
    cmap = f"""/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def
/CMapName /PagewingLong def
/CMapType 2 def
1 begincodespacerange
<00> <ff>
endcodespacerange
1 beginbfchar
<41> <{mapping}>
endbfchar
endcmap
CMapName currentdict /CMap defineresource pop
end
end
""".encode()
    content = b"BT /F1 24 Tf 50 700 Td (A) Tj ET"
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Resources << /Font << /F1 4 0 R >> >> /Contents 6 0 R >>",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 5 0 R >>",
        f"<< /Length {len(cmap)} >>\nstream\n".encode() + cmap + b"endstream",
        f"<< /Length {len(content)} >>\nstream\n".encode() + content + b"\nendstream",
    ]
    data = bytearray(b"%PDF-1.4\n")
    offsets = [0]
    for number, obj in enumerate(objects, 1):
        offsets.append(len(data))
        data.extend(f"{number} 0 obj\n".encode() + obj + b"\nendobj\n")
    xref = len(data)
    data.extend(f"xref\n0 {len(offsets)}\n0000000000 65535 f \n".encode())
    for offset in offsets[1:]:
        data.extend(f"{offset:010} 00000 n \n".encode())
    data.extend(f"trailer\n<< /Size {len(offsets)} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode())
    return data


if __name__ == "__main__":
    path = Path(__file__).resolve().parent.parent / "assets/selection-long-mapping.pdf"
    path.write_bytes(make_pdf())
    print(path)
