#!/usr/bin/env python3
"""Exercise document metadata, modal shortcuts, reload and document switching."""
import ctypes
import json
import os
from pathlib import Path
import re
import runpy
import signal
import subprocess
import sys
import tempfile
import time

SCRIPTS = Path(__file__).resolve().parent
desktop = runpy.run_path(str(SCRIPTS / "smoke-desktop.py"))
Keyboard, wait_for = desktop["Keyboard"], desktop["wait_for"]


def metadata_pdf(source, title):
    previous = int(re.findall(rb"startxref\s+(\d+)", source)[-1])
    size = int(re.findall(rb"/Size\s+(\d+)", source)[-1])
    title = (b"\xfe\xff" + title.encode("utf-16-be")).hex().encode()
    data = source + b"\n"
    offset = len(data)
    data += str(size).encode() + b" 0 obj\n<< /Title <" + title + b"> /Author (Author\x84Name) /Subject (Reading) /Creator (Folio smoke) /Keywords ( ) >>\nendobj\n"
    xref = len(data)
    data += f"xref\n{size} 1\n{offset:010} 00000 n \ntrailer\n<< /Size {size + 1} /Root 1 0 R /Info {size} 0 R /Prev {previous} >>\nstartxref\n{xref}\n%%EOF\n".encode()
    return data


def run(binary):
    with tempfile.TemporaryDirectory(prefix="folio-properties-") as directory:
        root = Path(directory)
        source = (SCRIPTS.parent / "assets/folio-guide.pdf").read_bytes()
        document, clean = root / "folio-smoke.pdf", root / "clean.pdf"
        document.write_bytes(metadata_pdf(source, "中文 😀 reading"))
        clean.write_bytes(source)
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/folio/library.json"
        config_path.parent.mkdir(parents=True)
        config_path.write_text(json.dumps({"recent": [{"path": str(clean), "page": 3}]}))
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)

            def book():
                try:
                    return json.loads(config_path.read_text())["recent"][0]
                except (OSError, ValueError, KeyError, IndexError):
                    return {}

            def expect(path, page):
                wait_for(lambda: book().get("path") == str(path) and book().get("page") == page, f"{path.name} page {page + 1}")

            def details():
                keyboard.key("y")
                return keyboard.clipboard()

            def recent(query):
                keyboard.key("p", ctrl=True)
                for character in query:
                    keyboard.key(character, delay=0)
                keyboard.key(0xFF0D)

            try:
                window = wait_for(keyboard.focus, "properties reader window")
                expect(document, 0)
                keyboard.key("s")
                assert "Title: 中文 😀 reading" in details(), "s opens document information"
                keyboard.key(0xFF1B)
                application.send_signal(signal.SIGSTOP)
                try:
                    for character in "99s":
                        keyboard.key(character, delay=0)
                    keyboard.key("j", shift=True, delay=0)
                    keyboard.key("r", delay=0)
                    keyboard.key("y", delay=0)
                finally:
                    application.send_signal(signal.SIGCONT)
                time.sleep(0.4)
                text = keyboard.clipboard()
                for row in [f"File: {document}", "Format: PDF 1.4", f"Size: {document.stat().st_size} bytes", "Pages: 6", "Page size: 600.0 × 800.0 pt", "Title: 中文 😀 reading", "Author: Author—Name", "Creator: Folio smoke"]:
                    assert row in text, (row, text)
                assert "Keywords:" not in text
                assert book()["page"] == 0 and book().get("rotation", 0) == 0
                keyboard.key(0xFF1B); keyboard.key("j", shift=True)
                expect(document, 1)
                keyboard.key("s")
                assert "Current page: ii · 2" in details()
                keyboard.key("f", ctrl=True)
                for character in "jkgquery":
                    keyboard.key(character, delay=0)
                keyboard.key("a", ctrl=True); keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == "jkgquery", "Find must replace properties and focus its input"
                keyboard.key(0xFF1B); keyboard.key("j", shift=True)
                expect(document, 2)
                keyboard.key("s")
                document.write_bytes(metadata_pdf(source, "Updated title"))
                keyboard.key("r", ctrl=True)
                def updated():
                    text = details()
                    return text if "Title: Updated title" in text else None
                wait_for(updated, "metadata refresh after reload")
                assert "Current page: 1 · 3" in details()
                recent("clean")
                expect(clean, 3)
                time.sleep(0.4)
                keyboard.key("s")
                text = details()
                assert f"File: {clean}" in text and "Title:" not in text and "Author:" not in text
                keyboard.key("w", ctrl=True)
                time.sleep(0.3)
                recent("folio-smoke")
                expect(document, 2)
                time.sleep(0.4)
                keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_uint, ctypes.c_uint]
                keyboard.x11.XResizeWindow(keyboard.display, window, 860, 540)
                keyboard.x11.XFlush(keyboard.display)
                keyboard.key("s")
                assert "Title: Updated title" in details()
                keyboard.key(0xFF1B); keyboard.key("j", shift=True)
                expect(document, 3)
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                print("PASS: document information shortcut, Unicode/PDFDocEncoding metadata, exact document details, same-frame modal input, prefix reset, Find focus, reload, clean document switch, close/reopen, compact window and graceful quit")
            except Exception:
                log.flush(); log.seek(0)
                print(log.read(), file=sys.stderr)
                if config_path.exists():
                    print(config_path.read_text(), file=sys.stderr)
                raise
            finally:
                if application.poll() is None:
                    application.send_signal(signal.SIGCONT)
                    application.terminate(); application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/folio").resolve())
