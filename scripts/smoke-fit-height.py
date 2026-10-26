#!/usr/bin/env python3
"""Check automatic height fit across wide pages, rotation, resize and reopen."""
import ctypes
import json
import os
from pathlib import Path
import runpy
import subprocess
import sys
import tempfile
import time

SCRIPTS = Path(__file__).resolve().parent
desktop = runpy.run_path(str(SCRIPTS / "smoke-desktop.py"))
Keyboard, wait_for = desktop["Keyboard"], desktop["wait_for"]


def run(binary):
    with tempfile.TemporaryDirectory(prefix="folio-fit-height-") as directory:
        root = Path(directory)
        document = root / "folio-smoke.pdf"
        # Equal-length substitution preserves all xref offsets. Remaining pages
        # retain their portrait dimensions to exercise mixed-size navigation.
        document.write_bytes((SCRIPTS.parent / "assets/folio-guide.pdf").read_bytes().replace(
            b"/MediaBox [0 0 600 800]", b"/MediaBox [0 0 900 200]", 1))
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/folio/library.json"
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)

            def book():
                try:
                    return json.loads(config_path.read_text())["recent"][0]
                except (OSError, ValueError, KeyError, IndexError):
                    return {}

            def expect(predicate, description):
                wait_for(lambda: predicate(book()) if book() else False, description)

            def custom():
                fit = book().get("fit")
                return fit.get("Custom", 0) if isinstance(fit, dict) else 0

            def right_edge():
                for letter in "999l":
                    keyboard.key(letter)

            def reopen():
                keyboard.key("p", ctrl=True)
                for letter in "folio-smoke":
                    keyboard.key(letter, delay=0)
                keyboard.key(0xFF0D)

            try:
                window = wait_for(keyboard.focus, "height-fit reader window")
                expect(lambda item: item["page"] == 0, "initial wide page")
                keyboard.key("e"); right_edge()
                expect(lambda item: item["fit"] == "Height" and item["offset"] == [1.0, 0.0], "wide page fills height and scrolls horizontally")
                keyboard.key("m"); keyboard.key("a")
                keyboard.key("j")
                expect(lambda item: item["page"] == 1 and item["fit"] == "Height", "vertical edge advances to a portrait page without losing height fit")
                keyboard.key(0xFF51, alt=True)
                expect(lambda item: item["page"] == 0 and item["fit"] == "Height" and item["offset"][0] == 1, "history restores the wide page and horizontal position")
                keyboard.key("+", shift=True)
                wait_for(lambda: custom() > 2, "zoom derives its scale from height fit")
                large = custom()
                keyboard.key("e")
                keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_uint, ctypes.c_uint]
                keyboard.x11.XResizeWindow(keyboard.display, window, 860, 540)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.4)
                keyboard.key("+", shift=True)
                wait_for(lambda: 0 < custom() < large - 1, "height fit adapts to the shorter window")
                small = custom()
                assert abs((large - small) - 280 / 200 * 1.2) < 0.02, (large, small)
                keyboard.key("e"); keyboard.key("r"); keyboard.key("+", shift=True)
                wait_for(lambda: 0 < custom() < small / 3, "height fit uses rotated dimensions")
                assert abs(small / custom() - 4.5) < 0.02
                keyboard.key("'"); keyboard.key("a")
                expect(lambda item: item["fit"] == "Height" and item["rotation"] == 0 and item["offset"][0] == 1, "mark restores automatic height fit")
                keyboard.key("a"); keyboard.key("/"); keyboard.key("e")
                keyboard.key("a", ctrl=True); keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == "e", "height shortcut must remain text in an input"
                wait_for(lambda: custom() == 1, "typing e preserves actual-size zoom")
                keyboard.key(0xFF1B); keyboard.key("e"); right_edge()
                expect(lambda item: item["fit"] == "Height" and item["offset"][0] == 1, "return to height fit after search")
                keyboard.key("w", ctrl=True)
                reopen()
                expect(lambda item: item["fit"] == "Height" and item["offset"][0] == 1, "recent reopen restores height fit")
                keyboard.key("+", shift=True)
                wait_for(lambda: custom() > 1, "reopened PDF remains interactive")
                keyboard.key("e")
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)
                wait_for(keyboard.focus, "restarted height-fit reader")
                time.sleep(0.5)
                keyboard.key("+", shift=True)
                wait_for(lambda: custom() > 1, "restart restores an automatic height fit usable for zooming")
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                print("PASS: wide/portrait pages, horizontal pan, height edge navigation, history/marks, resize and rotated scale, typing guard, recent reopen and restart")
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
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/folio").resolve())
