#!/usr/bin/env python3
"""Check hidden reader bars, mouse recovery and search drafts on real X11."""
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
CANVAS = (235, 239, 235)
F9, ESCAPE = 0xFFC6, 0xFF1B


def run(binary):
    for layout in ["single", "continuous", "facing"]:
        with tempfile.TemporaryDirectory(prefix="pagewing-reading-controls-") as directory:
            root = Path(directory)
            document = root / "pagewing-smoke.pdf"
            original = (SCRIPTS.parent / "assets/pagewing-guide.pdf").read_bytes()
            document.write_bytes(original)
            config = root / "config/pagewing"
            config.mkdir(parents=True)
            (config / "config.toml").write_text(
                f'[display]\nlayout = "{layout}"\nfit = 2.0\n')
            state_path = config / "library.json"
            environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                               XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
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

                def resize(width, height):
                    keyboard.x11.XResizeWindow(keyboard.display, window, width, height)
                    keyboard.x11.XFlush(keyboard.display)
                    time.sleep(0.3)

                def restored():
                    return keyboard.pixel((4, 4)) != CANVAS

                def copy_query():
                    keyboard.key("f", ctrl=True)
                    keyboard.key("a", ctrl=True)
                    keyboard.key("c", ctrl=True)
                    return keyboard.clipboard()

                try:
                    window = wait_for(keyboard.focus, "reading controls reader")
                    wait_for(lambda: book().get("path"), "loaded document")
                    resize(1180, 820)
                    for _ in range(4):
                        keyboard.key("j")
                    wait_for(lambda: book().get("offset", [0, 0])[1] > 0, "scrolled reading state")
                    before = book()
                    assert restored()
                    keyboard.key("f", ctrl=True)
                    for _ in range(7):
                        keyboard.key(0xFF09)
                    # Page owns focus. Process F9 before Enter can activate its
                    # now-hidden response, even when X11 delivers one batch.
                    application.send_signal(signal.SIGSTOP)
                    try:
                        _, status = os.waitpid(application.pid, os.WUNTRACED)
                        assert os.WIFSTOPPED(status)
                        keyboard.key(F9, delay=0)
                        keyboard.key(0xFF0D, delay=0)
                        keyboard.x11.XSync(keyboard.display, False)
                    finally:
                        application.send_signal(signal.SIGCONT)
                    wait_for(lambda: keyboard.pixel((4, 50)) == CANVAS, "same-batch hide before Page activation")
                    keyboard.key(ESCAPE)
                    wait_for(lambda: not restored(), "toolbar hidden")
                    for width, height in [(240, 240), (1180, 820), (240, 240)]:
                        resize(width, height)
                        assert not restored(), "resizing must retain hidden controls"
                    # The canvas remains the recovery anchor even at 240x240.
                    keyboard.click((180, 20))
                    wait_for(restored, "mouse recovery at minimum window size")
                    resize(1180, 820)
                    keyboard.key(F9)
                    wait_for(lambda: not restored(), "hide again before search")
                    keyboard.key("f", ctrl=True)
                    query = "hidden reader draft"
                    for char in query:
                        keyboard.key(char, delay=0.015)
                    keyboard.key(F9)
                    assert copy_query() == query, "reading shortcut must preserve typed text"
                    assert keyboard.pixel((4, 50)) == CANVAS, "search is visible above the hidden reader"
                    keyboard.click((1120, 62))
                    wait_for(lambda: keyboard.pixel((4, 50)) != CANVAS, "restore beside the search bar")
                    assert copy_query() == query, "mouse recovery retains the search draft"
                    keyboard.key(ESCAPE)
                    keyboard.key("q", ctrl=True)
                    assert application.wait(timeout=8) == 0
                    after = book()
                    for field in ["page", "fit", "rotation", "continuous", "facing"]:
                        assert after[field] == before[field], (layout, field, before, after)
                    assert all(abs(a - b) < 0.0001 for a, b in zip(before["offset"], after["offset"])), (layout, before, after)
                    assert document.read_bytes() == original
                    print(f"PASS: {layout}: hidden bars, 240x240 mouse recovery, search draft/input ownership, preserved page/view/offset, unchanged PDF")
                except Exception:
                    log.flush(); log.seek(0)
                    print(log.read(), file=sys.stderr)
                    print(book(), file=sys.stderr)
                    raise
                finally:
                    if application.poll() is None:
                        application.terminate()
                        application.wait(timeout=5)
                    keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
