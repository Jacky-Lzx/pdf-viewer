#!/usr/bin/env python3
"""Check reader bar focus identity, keyboard activation and resizing on X11."""
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
    with tempfile.TemporaryDirectory(prefix="pagewing-toolbar-focus-") as directory:
        root = Path(directory)
        document = root / "pagewing-smoke.pdf"
        original = (SCRIPTS.parent / "assets/pagewing-guide.pdf").read_bytes()
        document.write_bytes(original)
        config = root / "config/pagewing"
        config.mkdir(parents=True)
        # Space would visibly scroll this page if it leaked to reader commands.
        (config / "config.toml").write_text('[display]\nfit = 2.0\n')
        state_path = config / "library.json"
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        keyboard = Keyboard()
        keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong,
                                              ctypes.c_uint, ctypes.c_uint]
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment,
                                           stdout=log, stderr=log)

            def state():
                try:
                    return json.loads(state_path.read_text())
                except (OSError, ValueError):
                    return None

            def book():
                value = state()
                return value["recent"][0] if value and value.get("recent") else None

            def resize(width, height):
                keyboard.x11.XResizeWindow(keyboard.display, window, width, height)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)

            try:
                window = wait_for(keyboard.focus, "toolbar focus reader window")
                wait_for(book, "initial reading state")
                resize(1180, 820)
                keyboard.key("f", ctrl=True)
                query = "toolbar needle"
                for char in query:
                    keyboard.key(char, delay=0.015)
                # The last toolbar button precedes the search field in focus order.
                keyboard.key(0xFF09, shift=True)
                for size in [(900, 540), (240, 240), (1180, 820), (240, 240)]:
                    resize(*size)
                time.sleep(2.1)  # The reader persists dirty positions every two seconds.
                before = book()
                assert before is not None and before.get("bookmarks", []) == []
                assert state()["sidebar"] is False, "toolbar focus must not toggle the sidebar"
                keyboard.key(" ")

                def bookmarked():
                    value = book()
                    return value if value and value.get("bookmarks") == [0] else None

                after = wait_for(bookmarked, "Space bookmarks the retained toolbar target")
                for field in ["page", "fit", "rotation", "offset"]:
                    assert after[field] == before[field], f"toolbar activation changed {field}"
                assert state()["sidebar"] is False
                keyboard.key(0xFF09, shift=True)
                keyboard.key(0xFF0D)
                keyboard.key("a", ctrl=True)
                keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == query, "Shift+Tab/Enter returns to Find and retains query text"
                keyboard.key(0xFF1B)
                keyboard.key(0xFF09)
                wait_for(lambda: state() and state()["sidebar"], "Escape returns Tab to the reader shortcut")
                assert book()["page"] == 0 and book()["bookmarks"] == [0]
                keyboard.key(0xFF09)
                wait_for(lambda: state() and not state()["sidebar"], "hide sidebar before bottom controls")

                def focus_status(tabs):
                    resize(1180, 820)
                    keyboard.key("f", ctrl=True)
                    # Aa, Word, Prev, Next, Results, Close, Page, +, Zoom, -, View.
                    # Copy is disabled for this absent query.
                    for _ in range(tabs):
                        keyboard.key(0xFF09)
                    keyboard.key("4")  # Numeric status text appears before the controls.
                    for size in [(240, 240), (1180, 820), (240, 240)]:
                        resize(*size)
                    assert not state()["sidebar"], "bottom Tab must not toggle the sidebar"

                focus_status(7)
                keyboard.key(0xFF0D)
                keyboard.key("2")
                keyboard.key("a", ctrl=True)
                keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == "2", "retained Page button opens its editable dialog"
                keyboard.key(0xFF1B)
                assert book()["page"] == 0, "Page activation must not submit the reader count"

                focus_status(9)
                keyboard.key(" ")
                keyboard.key("a", ctrl=True)
                keyboard.key("c", ctrl=True)
                assert float(keyboard.clipboard()) == 200.0, "retained Zoom button opens the percentage dialog"
                keyboard.key(0xFF1B)

                focus_status(10)
                keyboard.key(" ")

                def zoomed_out():
                    value = book()
                    fit = value.get("fit") if value else None
                    return value if isinstance(fit, dict) and abs(fit.get("Custom", 0) - 2.0 / 1.2) < 1e-5 else None

                after_zoom = wait_for(zoomed_out, "Space applies one bottom-bar zoom step")
                assert after_zoom["page"] == 0 and after_zoom["rotation"] == before["rotation"]
                assert after_zoom["bookmarks"] == [0] and not state()["sidebar"]
                assert document.read_bytes() == original, "toolbar interaction must not write the PDF"
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                print("PASS: reader bar Tab/Shift+Tab, retained Bookmark/Page/Zoom/minus targets across status-prefix and wide/compact/minimum resizing, keyboard dialogs and one zoom step, no toolbar page/pan changes, Find/query retention, Escape restores reader Tab, unchanged PDF")
            except Exception:
                log.flush()
                log.seek(0)
                print(log.read(), file=sys.stderr)
                raise
            finally:
                if application.poll() is None:
                    application.terminate()
                    application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
