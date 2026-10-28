#!/usr/bin/env python3
"""Keep zoom, page and shortcut-help actions focused across native resizes."""
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
        keyboard.x11.XSync.argtypes = [ctypes.c_void_p, ctypes.c_int]
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

            def traverse(tabs, reverse=0, activation=None):
                # Real busy frames can receive several focus keys at once.
                application.send_signal(signal.SIGSTOP)
                try:
                    _, status = os.waitpid(application.pid, os.WUNTRACED)
                    assert os.WIFSTOPPED(status), "reader must stop before queuing Tabs"
                    for _ in range(tabs):
                        keyboard.key(TAB, delay=0)
                    for _ in range(reverse):
                        keyboard.key(TAB, shift=True, delay=0)
                    if activation is not None:
                        keyboard.key(activation, delay=0)
                    keyboard.x11.XSync(keyboard.display, False)
                finally:
                    application.send_signal(signal.SIGCONT)

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
                traverse(5, activation=SPACE)
                expect(0, 2.25)

                keyboard.key("z", ctrl=True, alt=True)
                text("175")
                traverse(6, reverse=1, activation=ENTER)
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

                keyboard.key(0xFFBE)  # F1.
                text("zoom 0")
                traverse(1)  # Copy shortcuts, with a single matching result.
                resize_focused_action()
                keyboard.key(ENTER)
                wait_for(lambda: "100% zoom" in keyboard.clipboard(),
                         "help copy retains focus through window resizing")
                expect(4, 1.75)
                # Copy returns to the filter. Close remains the second button.
                traverse(2)
                resize_focused_action()
                keyboard.key(SPACE)
                keyboard.key("k", shift=True)
                expect(3, 1.75)

                keyboard.key(0xFFBE)
                text("missing")
                traverse(1)  # Copy is disabled; Close follows the filter.
                resize_focused_action()
                keyboard.key(ENTER)
                keyboard.key("k", shift=True)
                expect(2, 1.75)
                assert application.poll() is None
                print("PASS: all zoom presets, Apply, Cancel, page Go/Cancel and help Copy/Close "
                      "with matching/empty results retain keyboard focus after batched Tabs; "
                      "reverse Tabs and activation in the same burst keep their order "
                      "through minimum, wide-short, narrow-tall and restored windows; Enter/Space "
                      "activate the original action and release dialog focus")
            except Exception:
                log.flush()
                log.seek(0)
                print(log.read(), file=sys.stderr)
                print(book(), file=sys.stderr)
                raise
            finally:
                if application.poll() is None:
                    application.send_signal(signal.SIGCONT)
                application.terminate()
                try:
                    application.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    application.kill()
                    application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
