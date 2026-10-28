#!/usr/bin/env python3
"""Exercise comment filtering, thread context and in-place saving on X11."""
import json
import os
from pathlib import Path
import runpy
import signal
import subprocess
import sys
import tempfile

SCRIPTS = Path(__file__).resolve().parent
desktop = runpy.run_path(str(SCRIPTS / "smoke-desktop.py"))
Keyboard, wait_for = desktop["Keyboard"], desktop["wait_for"]


def comments_pdf(rows=0):
    author = (b"\xfe\xff" + "Straße".encode("utf-16-be")).hex()
    extra = " ".join(f"{10 + index} 0 R" for index in range(rows))
    objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
        f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Annots [5 0 R 6 0 R 8 0 R 9 0 R {extra}] >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Annots [7 0 R] >>",
        "<< /Type /Annot /Subtype /Text /Rect [50 650 70 670] /T (Alice) /Contents (Root question) >>",
        f"<< /Type /Annot /Subtype /Text /Rect [50 650 70 670] /IRT 5 0 R /RT /R /T <{author}> /Contents (Nested needle jkg) >>",
        "<< /Type /Annot /Subtype /Highlight /Rect [50 620 110 670] "
        "/QuadPoints [50 650 110 650 50 670 110 670 50 620 110 620 50 640 110 640] "
        "/T (Bob) /Contents (Elsewhere comment) >>",
        "<< /Type /Annot /Subtype /Text /Rect [0 0 0 0] /IRT 6 0 R /T (Pagewing) /StateModel (Review) /State (Completed) /F 2 >>",
        "<< /Type /Annot /Subtype /Text /Rect [0 0 0 0] /IRT 6 0 R /T (Pagewing) /StateModel (Marked) /State (Marked) /F 2 >>",
    ]
    objects.extend(
        f"<< /Type /Annot /Subtype /Text /Rect [50 650 70 670] /T (Reviewer) /Contents (Row {index}) >>"
        for index in range(rows)
    )
    data = bytearray(b"%PDF-1.4\n")
    offsets = [0]
    for number, obj in enumerate(objects, 1):
        offsets.append(len(data))
        data.extend(f"{number} 0 obj\n{obj}\nendobj\n".encode())
    xref = len(data)
    data.extend(f"xref\n0 {len(offsets)}\n0000000000 65535 f \n".encode())
    for offset in offsets[1:]:
        data.extend(f"{offset:010} 00000 n \n".encode())
    data.extend(f"trailer\n<< /Root 1 0 R /Size {len(offsets)} >>\nstartxref\n{xref}\n%%EOF\n".encode())
    return bytes(data)


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-comments-") as directory:
        root = Path(directory)
        document = root / "pagewing-smoke.pdf"
        original = comments_pdf()
        document.write_bytes(original)
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        state = root / "config/pagewing/library.json"
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)

            def expect(page):
                def saved():
                    try:
                        return json.loads(state.read_text())["recent"][0]["page"] == page
                    except (OSError, ValueError, KeyError, IndexError):
                        return False
                wait_for(saved, f"reading page {page + 1}")

            def text(value, delay=0.01):
                for char in value:
                    keyboard.key(char.lower(), shift=char.isupper(), delay=delay)

            def copied():
                keyboard.key("a", ctrl=True)
                keyboard.key("c", ctrl=True)
                return keyboard.clipboard()

            def query(value):
                keyboard.click((1000, 114))
                keyboard.key("a", ctrl=True)
                text(value)

            try:
                wait_for(keyboard.focus, "comments reader window")
                expect(0)
                keyboard.key("m", ctrl=True, shift=True)
                query("elsewhere")
                assert copied() == "elsewhere", "filter input owns text editing shortcuts"
                for window in ("recent", "help", "properties"):
                    if window == "recent":
                        keyboard.key("p", ctrl=True)
                    else:
                        keyboard.click((500, 400))  # Return focus to the reader.
                        keyboard.key(0xFFBE if window == "help" else "s")
                    keyboard.key(0xFF1B)
                    query("elsewhere")
                    assert copied() == "elsewhere", f"Escape closes {window} before the underlying comment panel"
                    expect(0)
                keyboard.key(0xFF09, shift=True)  # Filter -> Copy.
                keyboard.key(0xFF0D)
                exported = keyboard.clipboard()
                for expected in ["Page 2 · Highlight", "Author: Bob", "Elsewhere comment"]:
                    assert expected in exported, (expected, exported)
                assert "Root question" not in exported
                keyboard.key(0xFF09)  # Copy -> filter, without toggling the left sidebar.
                assert copied() == "elsewhere", "Tab returns to filter text editing"
                keyboard.key(0xFF09, shift=True)
                keyboard.key(0x20)
                assert keyboard.clipboard() == exported, "Space activates Copy without paging"
                expect(0)
                assert document.read_bytes() == original, "copying comments leaves the PDF intact"
                query("elsewhere")
                expect(0)
                keyboard.key(0xFF0D)
                expect(1)
                assert copied() == "Elsewhere comment", "card opens the full comment on its page"
                application.send_signal(signal.SIGSTOP)
                try:
                    text("Updated elsewhere comment", delay=0)
                    keyboard.key("w", ctrl=True, delay=0)
                finally:
                    application.send_signal(signal.SIGCONT)
                assert copied() == "Updated elsewhere comment", "same-frame close retains the final unsaved text"
                assert document.read_bytes() == original, "draft protection does not write the PDF"
                application.send_signal(signal.SIGSTOP)
                try:
                    text("Saved elsewhere comment", delay=0)
                    keyboard.key(0xFF0D, ctrl=True, delay=0)
                    keyboard.key("r", ctrl=True, delay=0)
                finally:
                    application.send_signal(signal.SIGCONT)
                wait_for(lambda: document.read_bytes() != original, "comment saved in the original PDF")
                keyboard.click((1000, 114))
                assert copied() == "elsewhere", "saving keeps the comment filter"
                # Replacing the PDF precedes the asynchronous reload. Wait for
                # the new list to become interactive before checking its editor.
                def saved_comment():
                    keyboard.click((1000, 300))
                    return copied() == "Saved elsewhere comment"
                wait_for(saved_comment, "saved text survives the worker reload")
                query("jkg STRASSE")
                assert copied() == "jkg STRASSE", "Unicode folding query does not navigate or rotate"
                expect(1)
                keyboard.click((920, 180))  # Current page: reply lives on the other page.
                keyboard.key(0xFF1B)
                keyboard.key("k", shift=True)
                expect(0)
                keyboard.key("m", ctrl=True, shift=True)
                keyboard.click((1030, 145))  # Marked only; retain the root as context.
                query("jkg STRASSE")
                keyboard.click((1075, 84))
                exported = keyboard.clipboard()
                for expected in ["[Thread context]", "Root question", "Author: Straße", "Reply depth: 1", "Status: Completed / resolved", "Marked: yes", "Nested needle jkg"]:
                    assert expected in exported, (expected, exported)
                assert exported.index("Root question") < exported.index("Nested needle jkg")
                assert "Saved elsewhere comment" not in exported
                query("jkg STRASSE")
                keyboard.key("k", ctrl=True)
                keyboard.key(0xFF0D)
                assert copied() == "Root question", "keyboard selection can open retained parent context"
                query("jkg STRASSE")
                keyboard.key(0xFF0D)
                assert copied() == "Nested needle jkg", "filtered reply stays selectable with its parent context"
                query("jkg STRASSE")
                application.send_signal(signal.SIGSTOP)
                try:
                    keyboard.key("m", ctrl=True, shift=True, delay=0)
                    keyboard.key("j", shift=True, delay=0)
                finally:
                    application.send_signal(signal.SIGCONT)
                expect(1)
                print("PASS: native comment filters, Escape closes recent/help/properties before comments, Unicode author matching, current-page/marked combination, keyboard selection/Enter, parent context, full editor, same-frame unsaved text/close and final text/save/reload protection, multiline highlight read/edit/reload, filtered clipboard export with thread context, keyboard activation/focus traversal and page navigation")
            except Exception:
                log.flush()
                log.seek(0)
                print(log.read(), file=sys.stderr)
                raise
            finally:
                application.send_signal(signal.SIGCONT)
                application.terminate()
                application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
