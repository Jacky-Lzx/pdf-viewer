#!/usr/bin/env python3
"""Exercise counted percentage jumps and input isolation in the native reader."""
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


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-percentage-") as directory:
        root = Path(directory)
        document = root / "pagewing-smoke.pdf"
        source = (SCRIPTS.parent / "assets/pagewing-guide.pdf").read_bytes()
        document.write_bytes(source)
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/pagewing/library.json"
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)

            def book():
                try:
                    return json.loads(config_path.read_text())["recent"][0]
                except (OSError, ValueError, KeyError, IndexError):
                    return {}

            def expect(page, vertical=None):
                return wait_for(lambda: book() if book().get("page") == page and (
                    vertical is None or abs(book().get("offset", [0, 0])[1] - vertical) < 0.001
                ) else None, f"page {page + 1}, vertical {vertical}")

            def sequence(value, burst=False):
                if burst:
                    application.send_signal(signal.SIGSTOP)
                try:
                    for character in value:
                        keyboard.key(character.lower(), shift=character.isupper() or character in "%+?:$", delay=0 if burst else 0.025)
                finally:
                    if burst:
                        application.send_signal(signal.SIGCONT)

            try:
                window = wait_for(keyboard.focus, "percentage reader window")
                expect(0)
                for value, page in [("50%", 2), ("51%", 3), ("25%", 1), ("75%", 4),
                                    ("0%", 0), ("99%", 5), ("1%", 0), ("100%", 5),
                                    ("0%", 0), ("999999999999%", 5), ("0%", 0)]:
                    sequence(value)
                    expect(page)
                sequence("%J", burst=True)
                expect(1)
                sequence("25%J", burst=True)
                expect(2)
                sequence("75%K", burst=True)
                expect(3)
                sequence("0%w+++zbma", burst=True)
                original = expect(0, 1)
                sequence("50%", burst=True)
                moved = expect(2, 0)
                assert moved["fit"] == original["fit"], "percentage jump preserves zoom"
                keyboard.key(0xFF51, alt=True)
                restored = expect(0, 1)
                assert restored["fit"] == original["fit"]
                keyboard.key(0xFF53, alt=True)
                expect(2, 0)
                sequence("zb50%", burst=True)
                expect(2, 1)
                keyboard.key(0xFF51, alt=True)
                expect(0, 1)
                sequence("rp75%", burst=True)
                wait_for(lambda: book().get("page") == 4 and book().get("rotation") == 1, "rotated percentage jump")
                keyboard.key("f", ctrl=True)
                sequence("50%jz$")
                keyboard.key("a", ctrl=True); keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == "50%jz$", "percentage and dollar remain literal text in search"
                keyboard.key(0xFF1B)
                sequence(":25%")
                keyboard.key("a", ctrl=True); keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == "25%", "percentage remains literal text in page dialog"
                before = book()
                keyboard.key(0xFF0D)
                moved = expect(1)
                assert moved["fit"] == before["fit"] and moved["rotation"] == before["rotation"]
                keyboard.key(0xFF51, alt=True)
                expect(4)
                for value, page in [("0%", 0), ("51%", 3), ("101%", 5), ("75%", 4)]:
                    sequence(":" + value, burst=True)
                    keyboard.key(0xFF0D)
                    expect(page)
                keyboard.key("p", ctrl=True)
                sequence("50%")
                keyboard.key("a", ctrl=True); keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == "50%", "percentage remains literal text in recent filter"
                keyboard.key(0xFF1B)
                sequence("?25%")
                keyboard.key(0xFF1B)
                keyboard.key("j", shift=True)
                expect(5)
                sequence("75%")
                expect(4)
                keyboard.key("w", ctrl=True)
                keyboard.key("p", ctrl=True)
                sequence("pagewing-smoke")
                keyboard.key(0xFF0D)
                time.sleep(0.4)
                expect(4)
                keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_uint, ctypes.c_uint]
                keyboard.x11.XResizeWindow(keyboard.display, window, 240, 240)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)
                sequence(":50.5%", burst=True)
                keyboard.key(0xFF0D)
                time.sleep(0.3)
                keyboard.key("a", ctrl=True); keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == "50.5%", "invalid percentage remains editable at minimum size"
                sequence("50%")
                keyboard.key(0xFF0D)
                expect(2)
                # Keep xref offsets while making the last range a literal 5% label.
                document.write_bytes(source.replace(b"/S /D /P (A-)", b"      /P (5%)", 1))
                keyboard.key("r", ctrl=True)
                time.sleep(0.5)
                for value, page in [("5%", 0), ("=5%", 4)]:
                    sequence(":" + value, burst=True)
                    keyboard.key(0xFF0D)
                    expect(page)
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                print("PASS: rounded percentage page jumps, 0/100 and huge counts, bare %, same-frame count consumption, full view history, same-page pan, rotation, search/page/recent/help isolation, close/reopen persistence, page dialog submission, minimum-size validation and explicit percent PDF labels")
            except Exception:
                log.flush(); log.seek(0)
                print(log.read(), file=sys.stderr)
                if config_path.exists():
                    print(config_path.read_text(), file=sys.stderr)
                raise
            finally:
                if application.poll() is None:
                    application.terminate(); application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
