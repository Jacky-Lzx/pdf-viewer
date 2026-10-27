#!/usr/bin/env python3
"""Check keyboard selection and virtualized chapter scrolling in a real desktop."""
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
ENTER, ESCAPE = 0xFF0D, 0xFF1B


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-outline-keyboard-") as directory:
        root = Path(directory)
        fixture = runpy.run_path(str(SCRIPTS / "make-sample.py"))
        fixture["PAGES"][:] = [(f"Chapter {page + 1:03}", "CHAPTER", ["Keyboard chapter navigation."]) for page in range(80)]
        document = root / "pagewing-smoke.pdf"
        document.write_bytes(fixture["make_pdf"]())
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/pagewing/library.json"
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)

            def expect(page, description):
                def saved():
                    try:
                        book = json.loads(config_path.read_text())["recent"][0]
                        return book["page"] == page and book["rotation"] == 0
                    except (OSError, ValueError, KeyError, IndexError):
                        return False
                wait_for(saved, description)

            def burst(keys):
                application.send_signal(signal.SIGSTOP)
                try:
                    for symbol, modifiers in keys:
                        keyboard.key(symbol, delay=0, **modifiers)
                finally:
                    application.send_signal(signal.SIGCONT)
                time.sleep(0.4)

            def selected_row_visible():
                # Only inspect the chapter list, excluding headings, text input
                # and page list. Filled row backgrounds span all three points.
                filled = 0
                for y in range(188, 446, 2):
                    colors = [keyboard.pixel((x, y)) for x in (50, 80, 110)]
                    if all(green > red * 1.4 and green > blue * 1.1 for red, green, blue in colors):
                        filled += 1
                return filled >= 6

            try:
                window = wait_for(keyboard.focus, "long-outline reader window")
                expect(0, "initial document")
                keyboard.key("m", ctrl=True, shift=True)  # Use the full reading canvas.
                burst([("c", {}), ("k", {"ctrl": True}), ("j", {"ctrl": True})])
                keyboard.key(ENTER)
                expect(1, "same-frame up at the first chapter then down keeps event order")
                burst([("c", {}), ("a", {"ctrl": True})]
                      + [(letter, {}) for letter in "080 chapter"] + [(ENTER, {})])
                expect(79, "reordered chapter keywords match the same title before Enter")
                burst([("c", {}), ("a", {"ctrl": True}), (0xFF08, {})]
                      + [("j", {"ctrl": True})] * 79)
                wait_for(selected_row_visible, "last of 80 selected chapters is drawn")
                keyboard.key(ENTER)
                expect(79, "Enter follows the final selected chapter")
                burst([("c", {}), (0xFF54, {}), (0xFF52, {})])
                keyboard.key(ENTER)
                expect(78, "same-frame down at the last chapter then up keeps event order")
                keyboard.key("k", shift=True)
                expect(77, "chapter confirmation releases reading keys")
                keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_uint, ctypes.c_uint]
                keyboard.x11.XResizeWindow(keyboard.display, window, 860, 540)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)
                burst([("c", {})] + [(0xFF52, {})] * 79)
                wait_for(selected_row_visible, "first chapter selection is drawn in the compact window")
                keyboard.key(ENTER)
                expect(0, "first chapter remains reachable after virtual scrolling")
                burst([("c", {}), ("a", {"ctrl": True})] + [(letter, {}) for letter in "080 chapter"])
                wait_for(selected_row_visible, "filtering finds the final title without old row offsets")
                keyboard.key(ENTER)
                expect(79, "filtered last chapter follows its real destination")
                keyboard.key(ESCAPE)
                keyboard.wheel((200, 480), down=True, steps=25)
                burst([("c", {}), ("a", {"ctrl": True})] + [(letter, {}) for letter in "001 chapter"])
                wait_for(selected_row_visible, "chapter focus reveals the list after the outer sidebar was scrolled")
                keyboard.key(ENTER)
                expect(0, "chapter selection works after scrolling the outer sidebar")
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                print("PASS: 80 chapter rows, reordered multiword filters, first/last keyboard navigation, actual selection visibility, compact layout, filtering after scrolling and outer sidebar focus")
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
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
