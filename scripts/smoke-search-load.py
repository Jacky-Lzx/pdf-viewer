#!/usr/bin/env python3
"""Check capped, virtualized search results in a native window. Use xvfb-run -a."""
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


def run(binary):
    with tempfile.TemporaryDirectory(prefix="folio-search-load-") as directory:
        root = Path(directory)
        fixture = runpy.run_path(str(SCRIPTS / "make-sample.py"))
        fixture["PAGES"][:] = [(f"Search load {page + 1}", "SEARCH LOAD", [("dense " * 16).strip()] * 8) for page in range(80)]
        data = re.sub(rb"/Outlines \d+ 0 R", lambda match: b" " * len(match.group()), bytes(fixture["make_pdf"]()), count=1)
        document = root / "folio-smoke.pdf"
        document.write_bytes(data)
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/folio/library.json"
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)

            def expect(page, description):
                def saved():
                    try:
                        return json.loads(config_path.read_text())["recent"][0]["page"] == page
                    except (OSError, ValueError, KeyError, IndexError):
                        return False
                wait_for(saved, description)

            def selected_result_visible():
                # Require a filled selection across the search-results region,
                # excluding the page list and small green heading text.
                for y in range(220, 494, 2):
                    colors = [keyboard.pixel((x, y)) for x in (20, 80, 150)]
                    if all(green > red * 1.4 and green > blue * 1.1 for red, green, blue in colors):
                        return True
                return False

            try:
                window = wait_for(keyboard.focus, "search load reader window")
                expect(0, "initial reading state")
                keyboard.key("/")
                for letter in "dense":
                    keyboard.key(letter)
                time.sleep(4)
                keyboard.key(0xFF0D); keyboard.key(0xFF1B)
                keyboard.key("n", shift=True)
                expect(78, "10,000th match ends on page 79 rather than page 80")
                keyboard.key(0xFF09)
                wait_for(selected_result_visible, "final selected result is actually drawn in the sidebar")
                keyboard.key("n")
                expect(0, "next match wraps to the first of 10,000 results")
                wait_for(selected_result_visible, "first result is visible after wrapping")
                keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_uint, ctypes.c_uint]
                keyboard.x11.XResizeWindow(keyboard.display, window, 860, 540)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)
                keyboard.key("n", shift=True)
                expect(78, "last match in minimum window")
                wait_for(selected_result_visible, "last selected result fits the compact sidebar")
                print("PASS: 80 pages, 10,000 capped matches, last/first navigation and actually visible selections at normal/compact sizes")
            except Exception:
                log.flush(); log.seek(0)
                print(log.read(), file=sys.stderr)
                if config_path.exists():
                    print(config_path.read_text(), file=sys.stderr)
                raise
            finally:
                application.terminate(); application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/folio").resolve())
