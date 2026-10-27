#!/usr/bin/env python3
"""Exercise held text drags at viewport edges, including rotated pages."""
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
ESCAPE = 0xFF1B


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-selection-scroll-") as directory:
        root = Path(directory)
        document = root / "pagewing-smoke.pdf"
        document.write_bytes((SCRIPTS.parent / "assets/pagewing-guide.pdf").read_bytes())
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/pagewing/library.json"
        keyboard = Keyboard()
        keyboard.xtst.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
        keyboard.xtst.XTestFakeButtonEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)

            def book():
                try:
                    return json.loads(config_path.read_text())["recent"][0]
                except (OSError, ValueError, KeyError, IndexError):
                    return {}

            def expect(predicate, description):
                def matched():
                    value = book()
                    return value if value and predicate(value) else None
                return wait_for(matched, description)

            def keys(value):
                for character in value:
                    keyboard.key(character)

            def drag_hold(start, end, seconds):
                def move(position):
                    keyboard.xtst.XTestFakeMotionEvent(keyboard.display, -1, *position, 0)
                    keyboard.x11.XFlush(keyboard.display)
                move(start)
                keyboard.xtst.XTestFakeButtonEvent(keyboard.display, 1, 1, 0)
                keyboard.x11.XFlush(keyboard.display)
                try:
                    time.sleep(0.1)
                    for step in range(1, 11):
                        move(tuple(round(a + (b - a) * step / 10) for a, b in zip(start, end)))
                        time.sleep(0.025)
                    time.sleep(seconds)
                finally:
                    keyboard.xtst.XTestFakeButtonEvent(keyboard.display, 1, 0, 0)
                    keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.2)

            def select_native():
                keyboard.key("v")
                time.sleep(0.3)
                keyboard.click((170, 540)); keyboard.click((170, 540))
                keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == "native"
                time.sleep(0.4)  # Start a new drag after the double-click interval.

            try:
                wait_for(keyboard.focus, "selection auto-scroll reader")
                expect(lambda value: value["page"] == 0, "initial page")
                keyboard.key("w")
                expect(lambda value: value["fit"] == "Width", "page wider than the vertical viewport")
                select_native()
                drag_hold((170, 540), (430, 780), 0.5)
                stopped = expect(lambda value: 0.1 < value["offset"][1] < 0.8,
                                 "holding the pointer at the bottom scrolls without further mouse motion")
                time.sleep(0.5)
                assert book()["offset"] == stopped["offset"], "releasing the drag stops auto-scroll"
                keyboard.key(ESCAPE); keys("zt")
                expect(lambda value: value["offset"] == [0.0, 0.0], "return to page top")
                select_native()
                drag_hold((170, 540), (430, 780), 2.3)
                expect(lambda value: value["page"] == 0 and value["offset"][1] == 1,
                       "held drag reaches the bottom and does not turn the page")
                keyboard.key("c", ctrl=True)
                copied = keyboard.clipboard()
                assert copied.startswith("native PDF reader"), copied
                assert "PAGEWING /" in copied, copied

                # Extend a fresh drag from the footer all the way back up.
                time.sleep(0.4)
                drag_hold((180, 680), (120, 62), 2.3)
                expect(lambda value: value["page"] == 0 and value["offset"][1] == 0,
                       "reverse drag scrolls back to the first text")
                keyboard.key("y")
                copied = keyboard.clipboard()
                assert copied.startswith("WELCOME TO PAGEWING"), copied
                assert "native PDF reader" in copied, copied

                # Reading rotation makes horizontal overflow scrollable too.
                keyboard.key(ESCAPE); keys("ra")
                for _ in range(3):
                    keyboard.key("+", shift=True)
                keys("ztz0")
                expect(lambda value: value["rotation"] == 1 and value["offset"] == [0.0, 0.0],
                       "rotated enlarged page starts at its top left")
                keyboard.key("v")
                time.sleep(0.3)
                drag_hold((650, 300), (1170, 400), 1.4)
                expect(lambda value: value["page"] == 0 and value["offset"] == [1.0, 0.0],
                       "right-edge drag scrolls a rotated page on the horizontal axis")
                keyboard.key("c", ctrl=True)
                assert keyboard.clipboard().strip(), "rotated selection still copies text"

                keys("ztz0")
                expect(lambda value: value["offset"] == [0.0, 0.0], "reset before margin drag")
                drag_hold((5, 400), (1170, 400), 0.6)
                time.sleep(0.4)
                assert book()["offset"] == [0.0, 0.0], "a drag starting outside the page must not auto-scroll"
                keyboard.key(ESCAPE); keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                assert book()["offset"] == [0.0, 0.0]
                print("PASS: held bottom/top/right edge drags, release stops scrolling, page boundaries, copied extended/reversed text, rotated horizontal selection and margin-drag isolation")
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
