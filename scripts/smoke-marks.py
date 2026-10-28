#!/usr/bin/env python3
"""Check saved-view restoration and mark deletion in a narrow native sidebar."""
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
    with tempfile.TemporaryDirectory(prefix="pagewing-marks-") as directory:
        root = Path(directory)
        fixture = runpy.run_path(str(SCRIPTS / "make-sample.py"))
        fixture["PAGES"][:] = [(f"Page {page + 1}", "MARKS", ["Saved reading positions."])
                              for page in range(80)]
        original = re.sub(rb"/Outlines \d+ 0 R", lambda match: b" " * len(match.group()),
                          bytes(fixture["make_pdf"](page_label_prefix="Long appendix 中文😀 title ")),
                          count=1)
        document = root / "pagewing-smoke.pdf"
        document.write_bytes(original)
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/pagewing/library.json"
        config_path.parent.mkdir(parents=True)
        mark = {"page": 79, "fit": {"Custom": 1.75}, "rotation": 1,
                "continuous": False, "facing": False, "offset": [0.4, 0.6]}
        config_path.write_text(json.dumps({"sidebar": True, "recent": [
            {"path": str(document), "marks": {"z": mark}},
        ]}))
        keyboard = Keyboard()
        keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong,
                                              ctypes.c_uint, ctypes.c_uint]
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment,
                                           stdout=log, stderr=log)

            def book():
                try:
                    return json.loads(config_path.read_text())["recent"][0]
                except (OSError, ValueError, KeyError, IndexError):
                    return {}

            def expect(page, names, description):
                return wait_for(lambda: (current if (current := book()).get("page") == page
                                         and set(current.get("marks", {})) == set(names) else None),
                                description)

            try:
                window = wait_for(keyboard.focus, "mark reader window")
                expect(0, "z", "initial saved mark")
                keyboard.x11.XResizeWindow(keyboard.display, window, 240, 400)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.4)
                keyboard.click((40, 200))
                restored = expect(79, "z", "clicking the truncated mark restores its page")
                for field in ("fit", "rotation", "continuous", "facing"):
                    assert restored[field] == mark[field], (field, restored)
                keyboard.key("g")
                keyboard.key("g")
                expect(0, "z", "return to the first page before deletion")
                keyboard.click((92, 200))
                expect(0, "", "the reserved delete button removes the mark without navigating")
                keyboard.click((125, 382))
                recovered = expect(0, "z", "Undo mark restores the saved position without navigating")
                assert recovered["marks"]["z"] == mark, recovered
                keyboard.key("'")
                keyboard.key("z")
                restored = expect(79, "z", "the recovered mark still follows its original destination")
                for field in ("fit", "rotation", "continuous", "facing"):
                    assert restored[field] == mark[field], (field, restored)
                keyboard.key("g")
                keyboard.key("g")
                expect(0, "z", "return to the first page after following the recovered mark")
                keyboard.click((92, 200))
                expect(0, "", "delete the recovered mark before saving a new location")
                keyboard.key("m")
                keyboard.key("z")
                expect(0, "z", "the same mark can be saved again")
                keyboard.key("j", shift=True)
                expect(1, "z", "move away from the newly saved mark")
                keyboard.click((40, 200))
                expect(0, "z", "the recreated mark follows its new destination")
                keyboard.click((92, 200))
                expect(0, "", "the recreated mark can be deleted")
                keyboard.key("m")
                keyboard.key("z")
                expect(0, "z", "prepare a selected mark in the minimum window")
                keyboard.x11.XResizeWindow(keyboard.display, window, 240, 240)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.4)
                keyboard.wheel((60, 120), down=True)

                def selected_mark_y():
                    rows = []
                    for y in range(90, 200, 2):
                        colors = [keyboard.pixel((x, y)) for x in (20, 40, 60)]
                        if all(g > r + 8 and g > b + 6 for r, g, b in colors):
                            rows.append(y)
                    if rows:
                        first = [y for y in rows if y < rows[0] + 30]
                        if len(first) >= 6:
                            return (first[0] + first[-1]) // 2
                    return None

                row_y = wait_for(selected_mark_y, "scrolling exposes the selected mark in 240×240")
                keyboard.click((92, row_y))
                expect(0, "", "the minimum-size mark delete button remains usable after scrolling")
                assert document.read_bytes() == original, "marks do not modify the PDF"
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                print("PASS: 240-pixel-wide window, minimum-window scrolling, long Unicode page labels, truncated mark clicks, "
                      "saved zoom/rotation/layout, reserved delete action, unchanged page on deletion, "
                      "complete mark recovery without navigation, recovered destination, mark recreation, "
                      "persisted state and unchanged PDF")
            except Exception:
                log.flush()
                log.seek(0)
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
