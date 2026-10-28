#!/usr/bin/env python3
"""Check page-based current chapters and explicit reveal through real X11 input."""
import ctypes
import json
import os
from pathlib import Path
import runpy
import signal
import subprocess
import sys
import tempfile
import time

SCRIPTS = Path(__file__).resolve().parent
desktop = runpy.run_path(str(SCRIPTS / "smoke-desktop.py"))
Keyboard, wait_for = desktop["Keyboard"], desktop["wait_for"]


def fixture():
    count, chapters = 80, 20
    first = 5 + count * 2
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R /Outlines 4 0 R >>",
        f"<< /Type /Pages /Kids [{' '.join(f'{5 + page * 2} 0 R' for page in range(count))}] /Count {count} >>".encode(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        f"<< /Type /Outlines /First {first} 0 R /Last {first + chapters - 1} 0 R /Count {chapters} >>".encode(),
    ]
    for page in range(count):
        stream = f"1 1 1 rg 0 0 600 800 re f 0 0 0 rg BT /F1 20 Tf 48 700 Td (Page {page + 1}) Tj ET".encode()
        objects.append(f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Resources << /Font << /F1 3 0 R >> >> /Contents {6 + page * 2} 0 R >>".encode())
        objects.append(f"<< /Length {len(stream)} >>\nstream\n".encode() + stream + b"\nendstream")
    for chapter in range(chapters):
        previous = f"/Prev {first + chapter - 1} 0 R" if chapter else ""
        following = f"/Next {first + chapter + 1} 0 R" if chapter + 1 < chapters else ""
        objects.append(f"<< /Title (Chapter {chapter + 1:03}) /Parent 4 0 R {previous} {following} /Dest [{5 + chapter * 8} 0 R /Fit] >>".encode())
    output, offsets = bytearray(b"%PDF-1.4\n"), [0]
    for index, value in enumerate(objects, 1):
        offsets.append(len(output))
        output.extend(f"{index} 0 obj\n".encode() + value + b"\nendobj\n")
    xref = len(output)
    output.extend(f"xref\n0 {len(offsets)}\n0000000000 65535 f \n".encode())
    for offset in offsets[1:]:
        output.extend(f"{offset:010} 00000 n \n".encode())
    output.extend(f"trailer\n<< /Size {len(offsets)} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode())
    return output


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-current-chapter-") as directory:
        root = Path(directory)
        document = root / "pagewing-smoke.pdf"
        original = fixture()
        document.write_bytes(original)
        config = root / "config/pagewing"
        config.mkdir(parents=True)
        (config / "config.toml").write_text('[appearance]\nsidebar = true\n')
        state_path = config / "library.json"
        state_path.write_text(json.dumps({"recent": [{"path": str(document), "page": 45,
                                                    "fit": {"Custom": 2.0}, "offset": [0.2, 0.3]}]}))
        stamp = state_path.stat().st_mtime_ns
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        keyboard = Keyboard()
        keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_uint, ctypes.c_uint]
        keyboard.x11.XSync.argtypes = [ctypes.c_void_p, ctypes.c_int]
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)

            def book():
                try:
                    return json.loads(state_path.read_text())["recent"][0]
                except (OSError, ValueError, KeyError, IndexError):
                    return {}

            def expect(page):
                wait_for(lambda: book().get("page") == page, f"reading page {page + 1}")

            def type_text(text):
                for char in text:
                    keyboard.key(char, delay=0.015)

            def chapter_marker():
                filled = 0
                for y in range(220, 520, 2):
                    colors = [keyboard.pixel((x, y)) for x in [50, 80, 110]]
                    left = keyboard.pixel((20, y))
                    if all(g > r + 8 and g > b + 6 for r, g, b in colors) and not (left[1] > left[0] + 8 and left[1] > left[2] + 6):
                        filled += 1
                        if filled >= 6:
                            return True
                    else:
                        filled = 0
                return False

            try:
                window = wait_for(keyboard.focus, "current chapter reader")
                wait_for(lambda: state_path.stat().st_mtime_ns != stamp, "loaded reading snapshot")
                expect(45)
                before = book()
                keyboard.key("c"); type_text("absent")
                keyboard.click((100, 185))
                wait_for(chapter_marker, "Here reveals Chapter 012")
                time.sleep(2.2)
                assert book()["page"] == 45 and book()["fit"] == before["fit"]
                assert book()["offset"] == before["offset"], "Here must not move the PDF"
                keyboard.key(0xFF0D)
                expect(44)
                keyboard.key("j", shift=True)
                expect(45)
                wait_for(chapter_marker, "current chapter remains highlighted between start pages")
                keyboard.key("f", ctrl=True); type_text("reader draft")
                application.send_signal(signal.SIGSTOP)
                try:
                    _, status = os.waitpid(application.pid, os.WUNTRACED)
                    assert os.WIFSTOPPED(status)
                    keyboard.click((100, 227))
                    for char in "chapter 012":
                        keyboard.key(char, delay=0)
                    keyboard.x11.XSync(keyboard.display, False)
                finally:
                    application.send_signal(signal.SIGCONT)
                time.sleep(0.3)
                keyboard.key("a", ctrl=True); keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == "chapter 012", "same-batch Here click redirects later text"
                keyboard.key("f", ctrl=True)
                keyboard.key("a", ctrl=True); keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == "reader draft", "Here retains the search draft"
                keyboard.key(0xFF1B)
                keyboard.x11.XResizeWindow(keyboard.display, window, 240, 240)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)
                keyboard.key("c"); keyboard.key("a", ctrl=True); type_text("absent")
                # Clear precedes the filter; Here precedes Clear in this narrow
                # header, where the all-fold controls no longer fit.
                keyboard.key(0xFF09, shift=True); keyboard.key(0xFF09, shift=True)
                keyboard.key(" ")
                time.sleep(0.3)
                expect(45)
                keyboard.key(0xFF0D)
                expect(44)
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                assert document.read_bytes() == original
                print("PASS: current chapter between page starts, explicit mouse reveal clears filter without moving PDF, same-batch input handoff and retained search draft, 240x240 keyboard reveal/confirmation, unchanged PDF")
            except Exception:
                log.flush(); log.seek(0)
                print(log.read(), file=sys.stderr)
                print(book(), file=sys.stderr)
                raise
            finally:
                if application.poll() is None:
                    application.terminate(); application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
