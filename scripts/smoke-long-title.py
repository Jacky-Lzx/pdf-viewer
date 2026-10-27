#!/usr/bin/env python3
"""Exercise bounded UTF-16 chapter titles without changing page content."""
import ctypes
import json
import os
from pathlib import Path
import re
import runpy
import subprocess
import sys
import tempfile
import time

SCRIPTS = Path(__file__).resolve().parent
desktop = runpy.run_path(str(SCRIPTS / "smoke-desktop.py"))
Keyboard, wait_for = desktop["Keyboard"], desktop["wait_for"]


def make_pdf():
    source = (SCRIPTS.parent / "assets/pagewing-guide.pdf").read_bytes()
    title = "Straße — " + "中文😀𝄞" * 12000 + " after-title-limit"
    encoded = b"\xfe\xff" + title.encode("utf-16-be")
    replacement = b"/Title <" + encoded.hex().encode() + b">"
    objects = re.findall(rb"(?m)^(\d+) 0 obj\n(.*?)\nendobj\n", source, re.S)
    assert objects
    output, offsets = bytearray(b"%PDF-1.4\n"), [0]
    replaced = 0
    for index, (number, value) in enumerate(objects, 1):
        assert int(number) == index
        replaced += value.count(b"/Title (Make yourself at home.)")
        value = value.replace(b"/Title (Make yourself at home.)", replacement)
        offsets.append(len(output))
        output.extend(number + b" 0 obj\n" + value + b"\nendobj\n")
    assert replaced == 1
    xref = len(output)
    output.extend(f"xref\n0 {len(offsets)}\n0000000000 65535 f \n".encode())
    for offset in offsets[1:]:
        output.extend(f"{offset:010} 00000 n \n".encode())
    output.extend(f"trailer\n<< /Size {len(offsets)} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode())
    return output


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-long-title-") as directory:
        root = Path(directory)
        document = root / "pagewing-smoke.pdf"
        document.write_bytes(make_pdf())
        config = root / "config/pagewing/library.json"
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment,
                                           stdout=log, stderr=log)

            def expect(page, description):
                def saved():
                    try:
                        return json.loads(config.read_text())["recent"][0]["page"] == page
                    except (OSError, ValueError, KeyError, IndexError):
                        return False
                wait_for(saved, description)

            def chapter(query):
                keyboard.key("c")
                keyboard.key("a", ctrl=True)
                for letter in query:
                    keyboard.key(letter, delay=0)
                keyboard.key(0xFF0D)

            try:
                window = wait_for(keyboard.focus, "reader with a long UTF-16 chapter title")
                expect(0, "document opened")
                keyboard.key("m", ctrl=True, shift=True)  # Use the full reading canvas.
                chapter("strasse")
                expect(2, "Unicode title prefix retains its actual destination")
                keyboard.key("y")
                assert "Make yourself at home." in keyboard.clipboard(), "page text stays intact"
                keyboard.key("j", shift=True)
                expect(3, "leave the long title's destination before checking the discarded suffix")
                chapter("after-title-limit")
                keyboard.key(0xFF1B)
                keyboard.key("j", shift=True)
                expect(4, "discarded title suffix does not match and Escape restores Vim navigation")
                keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_uint, ctypes.c_uint]
                keyboard.x11.XResizeWindow(keyboard.display, window, 860, 540)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)
                chapter("strasse")
                expect(2, "bounded title remains usable in the compact window")
                keyboard.key("k", shift=True)
                expect(1, "chapter confirmation releases reading keys")
                keyboard.key("r", ctrl=True)
                time.sleep(0.5)
                chapter("strasse")
                expect(2, "reload rebuilds the bounded title and its destination")
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                print("PASS: long UTF-16 title, Unicode prefix lookup, discarded suffix, intact page text, compact layout, Vim focus, reload and graceful quit")
            except Exception:
                log.flush(); log.seek(0)
                print(log.read(), file=sys.stderr)
                if config.exists():
                    print(config.read_text(), file=sys.stderr)
                raise
            finally:
                if application.poll() is None:
                    application.terminate(); application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
