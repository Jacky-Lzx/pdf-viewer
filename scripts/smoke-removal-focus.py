#!/usr/bin/env python3
"""Keep keyboard deletion and subsequent native input on surviving sidebar rows."""
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
ENTER, ESCAPE, TAB, SPACE = 0xFF0D, 0xFF1B, 0xFF09, 0x20


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-removal-focus-") as directory:
        root = Path(directory)
        fixture = runpy.run_path(str(SCRIPTS / "make-sample.py"))
        fixture["PAGES"][:] = [(f"Chapter {page + 1:03}", "REMOVAL FOCUS", ["Keyboard sidebar edits."])
                              for page in range(80)]
        document = root / "pagewing-smoke.pdf"
        original = bytes(fixture["make_pdf"]())
        document.write_bytes(original)
        config = root / "config/pagewing"
        config.mkdir(parents=True)
        state_path = config / "library.json"
        mark = {"page": 40, "fit": {"Custom": 1.75}, "rotation": 1,
                "continuous": False, "facing": False, "offset": [0.4, 0.6]}
        state_path.write_text(json.dumps({"sidebar": True, "recent": [{
            "path": str(document), "bookmarks": [0, 2, 4, 6],
            "marks": {name: mark for name in "amz"},
        }]}))
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        keyboard = Keyboard()
        keyboard.x11.XSync.argtypes = [ctypes.c_void_p, ctypes.c_int]
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

            def expect(page, bookmarks, marks, description):
                return wait_for(lambda: (current if (current := book()).get("page") == page
                                         and current.get("bookmarks") == bookmarks
                                         and set(current.get("marks", {})) == set(marks) else None),
                                description)

            def focus_delete(tabs):
                keyboard.key("c")
                keyboard.key("a", ctrl=True)
                for letter in "001":
                    keyboard.key(letter)
                for _ in range(tabs):
                    keyboard.key(TAB)

            def burst(keys, resize=None):
                application.send_signal(signal.SIGSTOP)
                try:
                    _, status = os.waitpid(application.pid, os.WUNTRACED)
                    assert os.WIFSTOPPED(status)
                    if resize:
                        keyboard.x11.XResizeWindow(keyboard.display, window, *resize)
                    for key in keys:
                        keyboard.key(key, delay=0)
                    keyboard.x11.XSync(keyboard.display, False)
                finally:
                    application.send_signal(signal.SIGCONT)
                time.sleep(0.4)

            try:
                window = wait_for(keyboard.focus, "sidebar deletion reader window")
                expect(0, [0, 2, 4, 6], "amz", "initial sidebar state")
                # The filter is followed by the chapter, then alternating delete/jump controls.
                focus_delete(4)
                burst([ENTER, ENTER])
                expect(4, [0, 4, 6], "amz", "delete then Enter follows the next bookmark")
                keyboard.key(ESCAPE)
                keyboard.key("g"); keyboard.key("g")
                expect(0, [0, 4, 6], "amz", "return to reading without sidebar focus")
                focus_delete(6)
                burst([SPACE, TAB, ENTER])
                expect(0, [0, 4], "mz", "last bookmark deletion, Tab and Enter reach the first mark delete")
                keyboard.key(ESCAPE)
                focus_delete(8)
                burst([ENTER, ENTER], resize=(240, 240))
                restored = expect(40, [0, 4], "m", "last mark deletion follows the previous saved view after resizing")
                for field in ("fit", "rotation", "continuous", "facing"):
                    assert restored[field] == mark[field], (field, restored)
                keyboard.key(TAB, shift=True)
                burst([SPACE])
                expect(40, [0, 4], "", "deleting the sole remaining mark keeps the reading page")
                keyboard.key(TAB)
                wait_for(lambda: json.loads(state_path.read_text()).get("sidebar") is False,
                         "empty mark list releases focus so Tab returns to the reader")
                assert document.read_bytes() == original, "sidebar edits leave the PDF unchanged"
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                print("PASS: keyboard deletion, next/previous jump focus, same-batch Enter, Space/Tab/Enter, "
                      "cross-list focus, minimum-window resize, saved view restoration, empty-list focus release and unchanged PDF")
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
