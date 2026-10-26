#!/usr/bin/env python3
"""Generate a small, deterministic PDF for manual checks and regression tests."""
from pathlib import Path


def pdf_string(text):
    return text.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")


PAGES = [
    ("A little room to read.", "WELCOME TO FOLIO", [
        "A native PDF reader, written in Rust.",
        "Designed for Linux and macOS. Made for your keyboard.",
        "", "Press j and k to scroll. Press J and K to turn pages.",
        "Type gg for the first page, or G for the last.",
        "Type 3G to jump directly to page three.",
        "", "Press ? whenever you want the keyboard reference.",
    ]),
    ("Find your way.", "NAVIGATION", [
        "j / k / h / l     Scroll in four directions.",
        "J / K             Next / previous page.",
        "gg / G            First / last page.",
        "42G               Jump to page 42.",
        "Ctrl+d / Ctrl+u   Move half a screen.",
        "Space             Move down a screen.",
        "", "Drag the page to pan. Double-click to fit the page.",
    ]),
    ("Make yourself at home.", "READING", [
        "p                 Fit the whole page.",
        "w                 Fit the page width.",
        "+ / -             Zoom in / out.",
        "r / Shift+r       Rotate in either direction.",
        "d                 Switch the interface theme.",
        "i                 Invert the page for night reading.",
        "", "Folio remembers your page, scroll, zoom and rotation.",
    ]),
    ("A thought worth keeping.", "SEARCH AND BOOKMARKS", [
        "/                 Search the document.",
        "n / N             Next / previous search result.",
        "b                 Add or remove a bookmark.",
        "ma / 'a           Save / restore named position a.",
        "Tab               Show the sidebar.",
        "y                 Copy all text on the page.",
        "v, drag, then y   Select and copy part of the text.",
        "", "Try searching for Rust. Matches are highlighted on the page.",
        "Search is case-insensitive and understands Unicode text.",
    ]),
    ("Keep the flow.", "UNDER THE HOOD", [
        "Folio renders PDFs with hayro, a pure Rust PDF engine.",
        "Rendering and search run on a background worker.",
        "Nearby pages are prepared before you need them.",
        "A bounded cache keeps recently viewed pages ready.",
        "", "No PDFium or Poppler installation is required.",
        "The interface sleeps when idle to conserve energy.",
    ]),
    ("Your next good read.", "READY WHEN YOU ARE", [
        "Press o to open your own PDF.",
        "Or drag a document into the window.",
        "", "This guide is also a rendering and text-search test fixture.",
        "It contains vector shapes, text and multiple pages.",
        "", "Built with Rust. Read at your own pace.",
    ]),
]


def make_pdf():
    objects = [b"<< /Type /Catalog /Pages 2 0 R >>", b"", b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>", b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>"]
    kids = []
    for index, (title, eyebrow, lines) in enumerate(PAGES):
        page_id = len(objects) + 1
        kids.append(f"{page_id} 0 R")
        objects.append(f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents {page_id + 1} 0 R >>".encode())
        ops = ["0.984 0.980 0.965 rg 0 0 600 800 re f", "0.16 0.45 0.36 rg 48 708 48 5 re f"]
        def text(x, y, size, text, bold=False, color="0.16 0.18 0.18"):
            ops.append(f"{color} rg BT /{'F2' if bold else 'F1'} {size} Tf {x} {y} Td ({pdf_string(text)}) Tj ET")
        text(48, 675, 11, eyebrow, True, "0.16 0.45 0.36")
        if index == 0:
            text(48, 160, 12, "Read the navigation guide", color="0.16 0.45 0.36")
            text(330, 160, 12, "Language website", color="0.16 0.45 0.36")
            page = objects[page_id - 1].decode()
            links = "/Annots [<< /Type /Annot /Subtype /Link /Rect [48 150 275 178] /Border [0 0 0] /Dest [7 0 R /Fit] >> << /Type /Annot /Subtype /Link /Rect [330 150 505 178] /Border [0 0 0] /A << /S /URI /URI (https://www.rust-lang.org/) >> >>]"
            objects[page_id - 1] = (page[:-2] + links + " >>").encode()
        text(48, 620, 29, title, True)
        for row, line in enumerate(lines):
            text(48, 555 - row * 28, 12, line)
        ops.append("0.82 0.83 0.80 RG 0.5 w 48 80 m 552 80 l S")
        text(48, 55, 10, "FOLIO / A KEYBOARD-DRIVEN PDF READER", color="0.42 0.46 0.43")
        text(535, 55, 10, f"{index + 1:02}")
        stream = "\n".join(ops).encode()
        objects.append(f"<< /Length {len(stream)} >>\nstream\n".encode() + stream + b"\nendstream")
    objects[1] = f"<< /Type /Pages /Kids [{' '.join(kids)}] /Count {len(kids)} >>".encode()
    outline_id = len(objects) + 1
    first_id = outline_id + 1
    objects[0] = f"<< /Type /Catalog /Pages 2 0 R /Outlines {outline_id} 0 R /Dests << /reading [9 0 R /Fit] >> /Names << /Dests << /Names [(search) [11 0 R /Fit]] >> >> >>".encode()
    objects.append(f"<< /Type /Outlines /First {first_id} 0 R /Last {first_id + len(PAGES) - 1} 0 R /Count {len(PAGES)} >>".encode())
    for index, (title, _, _) in enumerate(PAGES):
        previous = f"/Prev {first_id + index - 1} 0 R" if index else ""
        following = f"/Next {first_id + index + 1} 0 R" if index + 1 < len(PAGES) else ""
        dest = f"/Dest [{5 + index * 2} 0 R /Fit]"
        if index == 1:
            dest = "/A << /S /GoTo /D [7 0 R /Fit] >>"
        if index == 2:
            dest = "/Dest /reading"
        if index == 3:
            dest = "/Dest (search)"
        objects.append(f"<< /Title ({pdf_string(title)}) /Parent {outline_id} 0 R {previous} {following} {dest} >>".encode())
    output = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
    offsets = [0]
    for number, obj in enumerate(objects, 1):
        offsets.append(len(output))
        output.extend(f"{number} 0 obj\n".encode() + obj + b"\nendobj\n")
    xref = len(output)
    output.extend(f"xref\n0 {len(offsets)}\n0000000000 65535 f \n".encode())
    for offset in offsets[1:]:
        output.extend(f"{offset:010} 00000 n \n".encode())
    output.extend(f"trailer\n<< /Size {len(offsets)} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode())
    return output


if __name__ == "__main__":
    destination = Path(__file__).resolve().parent.parent / "assets" / "folio-guide.pdf"
    destination.write_bytes(make_pdf())
    print(destination)
