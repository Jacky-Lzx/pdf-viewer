#!/usr/bin/env python3
"""Browse virtual page lists and previews without changing the reading view."""
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
ENTER, ESCAPE, TAB = 0xFF0D, 0xFF1B, 0xFF09
HOME, END, UP, DOWN, PAGE_DOWN = 0xFF50, 0xFF57, 0xFF52, 0xFF54, 0xFF56


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-page-keyboard-") as directory:
        root = Path(directory)
        fixture = runpy.run_path(str(SCRIPTS / "make-sample.py"))
        fixture["PAGES"][:] = [
            (f"Chapter {page + 1:03}", "PAGE KEYBOARD", [f"Page content {page + 1:03}"])
            for page in range(80)
        ]
        document = root / "pagewing-smoke.pdf"
        original = bytes(fixture["make_pdf"]())
        document.write_bytes(original)
        config = root / "config/pagewing"
        config.mkdir(parents=True)
        (config / "config.toml").write_text("[appearance]\nsidebar = true\nthumbnails = false\n")
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

            def page():
                try:
                    return json.loads(state_path.read_text())["recent"][0]["page"]
                except (OSError, ValueError, KeyError, IndexError):
                    return None

            def expect(target, description):
                wait_for(lambda: page() == target, description)

            def page_between(first, last):
                current = page()
                return current if current is not None and first <= current < last else None

            def resize(width, height):
                keyboard.x11.XResizeWindow(keyboard.display, window, width, height)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)

            def burst(keys):
                application.send_signal(signal.SIGSTOP)
                try:
                    _, status = os.waitpid(application.pid, os.WUNTRACED)
                    assert os.WIFSTOPPED(status)
                    for key in keys:
                        keyboard.key(key, delay=0)
                    keyboard.x11.XSync(keyboard.display, False)
                finally:
                    application.send_signal(signal.SIGCONT)
                time.sleep(0.4)

            def focus_second_page():
                keyboard.key("c")
                keyboard.key("a", ctrl=True)
                for character in "001":
                    keyboard.key(character)
                # Clear filter, one chapter, List, Previews, Page 1, Page 2.
                for _ in range(6):
                    keyboard.key(TAB)
                keyboard.key(ENTER)
                expect(1, "keyboard focus identifies the second page row")
                keyboard.key("g"); keyboard.key("g")
                expect(0, "reader shortcuts retain the focused page row")

            try:
                window = wait_for(keyboard.focus, "page-keyboard reader window")
                expect(0, "initial reading page")
                for previews in (False, True):
                    resize(1180, 820)
                    if previews:
                        keyboard.key("t")
                    keyboard.key("g"); keyboard.key("g")
                    expect(0, "first page before browsing a page list")
                    focus_second_page()
                    burst([END, UP])
                    time.sleep(1.5)
                    assert page() == 0, "End/Up must browse focus without turning the PDF page"
                    resize(240, 240)
                    keyboard.key(ENTER)
                    expect(78, "resizing preserves the offscreen page target before Enter")
                    burst([HOME, DOWN, ENTER])
                    expect(1, "queued movements finish before activation in the smallest window")
                    burst([HOME, UP, " "])
                    expect(0, "Up clamps at the first row and Space follows it")
                    burst([END, DOWN, " "])
                    expect(79, "Down clamps at the last row and Space follows it")
                    burst([HOME, PAGE_DOWN, ENTER])
                    small = wait_for(lambda: page_between(1, 79),
                                     "PageDown follows a later row in the short viewport")
                    resize(1180, 820)
                    burst([HOME, PAGE_DOWN, ENTER])
                    if not previews:
                        wait_for(lambda: page_between(small + 1, 79),
                                 "the taller page list moves by more rows")
                    else:
                        expect(1, "the tall preview viewport contains one full card per page step")
                    keyboard.key(ESCAPE)
                    keyboard.key(END)
                    expect(79, "Escape returns End to document navigation")
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                assert document.read_bytes() == original
                print("PASS: list/preview virtual navigation, offscreen first/last and boundaries, ordered native batches, Enter/Space, 240x240 resizing, viewport paging and Escape to reading")
            except Exception:
                log.flush(); log.seek(0)
                print(log.read(), file=sys.stderr)
                if state_path.exists():
                    print(state_path.read_text(), file=sys.stderr)
                raise
            finally:
                if application.poll() is None:
                    application.terminate(); application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
