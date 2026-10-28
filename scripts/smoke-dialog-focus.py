#!/usr/bin/env python3
"""Keep zoom presets and page dialog actions focused across native resizes."""
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
ENTER, SPACE, TAB = 0xFF0D, 0x20, 0xFF09


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-dialog-focus-") as directory:
        root = Path(directory)
        document = root / "pagewing-smoke.pdf"
        document.write_bytes((SCRIPTS.parent / "assets/pagewing-guide.pdf").read_bytes())
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        state_path = root / "config/pagewing/library.json"
        keyboard = Keyboard()
        keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong,
                                              ctypes.c_uint, ctypes.c_uint]
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment,
                                           stdout=log, stderr=log)

            def book():
                try:
                    return json.loads(state_path.read_text())["recent"][0]
                except (OSError, ValueError, KeyError, IndexError):
                    return {}

            def expect(page, scale):
                def matches():
                    item = book()
                    return item.get("page") == page and item.get("fit") == {"Custom": scale}
                wait_for(matches, f"dialog activation: page {page + 1}, zoom {scale}")

            def text(value):
                for char in value:
                    keyboard.key(char)

            def traverse(tabs):
                for _ in range(tabs):
                    keyboard.key(TAB)

            def resize(size):
                keyboard.x11.XResizeWindow(keyboard.display, window, *size)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.2)

            def resize_focused_action():
                for size in [(240, 240), (1180, 240), (240, 820), (1180, 820)]:
                    resize(size)

            try:
                window = wait_for(keyboard.focus, "dialog focus reader")
                wait_for(lambda: book().get("fit") == "Page", "loaded document")
                for tabs, scale in enumerate([0.5, 1.0, 1.5, 2.0], start=1):
                    keyboard.key("z", ctrl=True, alt=True)
                    text("175")
                    traverse(tabs)
                    resize_focused_action()
                    keyboard.key(ENTER if tabs % 2 else SPACE)
                    expect(0, scale)

                keyboard.key("z", ctrl=True, alt=True)
                text("175")
                traverse(5)  # Apply.
                resize_focused_action()
                keyboard.key(ENTER)
                expect(0, 1.75)

                keyboard.key("z", ctrl=True, alt=True)
                text("225")
                traverse(6)  # Cancel must not apply the typed percentage.
                resize_focused_action()
                keyboard.key(SPACE)
                keyboard.key("j", shift=True)
                expect(1, 1.75)

                keyboard.key(":", shift=True)
                text("4")
                traverse(1)  # Go.
                resize_focused_action()
                keyboard.key(SPACE)
                expect(3, 1.75)

                keyboard.key(":", shift=True)
                text("6")
                traverse(2)  # Cancel must not navigate to page 6.
                resize_focused_action()
                keyboard.key(ENTER)
                keyboard.key("j", shift=True)
                expect(4, 1.75)
                assert application.poll() is None
                print("PASS: all zoom presets, Apply, Cancel and page Go/Cancel retain keyboard focus "
                      "through minimum, wide-short, narrow-tall and restored windows; Enter/Space "
                      "activate the original action and release dialog focus")
            except Exception:
                log.flush()
                log.seek(0)
                print(log.read(), file=sys.stderr)
                print(book(), file=sys.stderr)
                raise
            finally:
                application.terminate()
                try:
                    application.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    application.kill()
                    application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
