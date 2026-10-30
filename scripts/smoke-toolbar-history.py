#!/usr/bin/env python3
"""Exercise toolbar history, full-view restoration and focus across resizing."""
import json
import os
from pathlib import Path
import runpy
import subprocess
import sys
import tempfile

SCRIPTS = Path(__file__).resolve().parent
desktop = runpy.run_path(str(SCRIPTS / "smoke-desktop.py"))
Keyboard, wait_for = desktop["Keyboard"], desktop["wait_for"]


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-toolbar-history-") as directory:
        root = Path(directory)
        document = root / "pagewing-smoke.pdf"
        original = (SCRIPTS.parent / "assets/pagewing-guide.pdf").read_bytes()
        document.write_bytes(original)
        state_path = root / "config/pagewing/library.json"
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment,
                                           stdout=log, stderr=log)

            def book():
                try:
                    return json.loads(state_path.read_text())["recent"][0]
                except (OSError, ValueError, KeyError, IndexError):
                    return None

            def expect(predicate, description):
                def saved():
                    item = book()
                    return item if item and predicate(item) else None
                return wait_for(saved, description)

            def sequence(text):
                for char in text:
                    keyboard.key(char.lower(), shift=char.isupper() or char == "+", delay=0.025)

            def resize(width, height):
                keyboard.resize_reader(window, width, height)

            def history(activation, narrow=False):
                keyboard.key("f", ctrl=True)
                # Bookmark, Find, Theme, Help, Plugins, Hide controls, then the
                # sole enabled history button. Enter must not hide controls.
                for _ in range(7):
                    keyboard.key(0xFF09, shift=True)
                if narrow:
                    for size in [(240, 240), (1180, 820), (240, 240)]:
                        resize(*size)
                keyboard.key(activation)

            def restored(view, description):
                return expect(lambda item: all(item.get(key) == view.get(key)
                                               for key in ("page", "fit", "rotation", "continuous", "facing"))
                              and all(abs(a - b) < 0.001
                                      for a, b in zip(item["offset"], view["offset"])), description)

            try:
                window = wait_for(keyboard.focus, "toolbar history reader window")
                expect(lambda item: item["page"] == 0, "initial document")
                resize(1180, 820)
                sequence("wzb")
                first = expect(lambda item: item["fit"] == "Width" and item["offset"][1] == 1,
                               "first full reading view")
                sequence("Jr+zz")
                second = expect(lambda item: item["page"] == 1 and item["rotation"] == 1
                                and isinstance(item["fit"], dict)
                                and abs(item["offset"][1] - 0.5) < 0.001,
                                "second rotated and zoomed view")
                keyboard.key("f", ctrl=True)
                query = "toolbar history needle"
                sequence(query)
                history(0xFF0D, narrow=True)
                # At 240px the restored fit has different geometry; compare
                # normalized position only after returning to the original size.
                resize(1180, 820)
                restored(first, "Enter restores Back after minimum-size resizing")
                history(" ", narrow=True)
                resize(1180, 820)
                restored(second, "Space restores Forward without scrolling")
                keyboard.key("f", ctrl=True)
                keyboard.key("a", ctrl=True)
                keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == query, "history must retain the search query"
                keyboard.key(0xFF1B)
                # A new jump after Back clears Forward. With only Back enabled,
                # the same focus path must return to the original full view.
                history(0xFF0D)
                restored(first, "Back remains available after search")
                keyboard.key(0xFF1B)
                sequence("2J")
                expect(lambda item: item["page"] == 2, "new jump after Back")
                history(0xFF0D)
                restored(first, "new jump clears Forward and exposes only Back")
                keyboard.key("w", ctrl=True)
                keyboard.key("p", ctrl=True)
                sequence("pagewing-smoke")
                keyboard.key(0xFF0D)
                restored(first, "close and reopen preserve the view")
                assert document.read_bytes() == original, "history must not write the PDF"
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                print("PASS: toolbar Back/Forward, Enter/Space, full-view and normalized-scroll restoration, retained focus at minimum size, query retention, new-jump forward clearing, close/reopen, unchanged PDF")
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
