#!/usr/bin/env python3
"""Exercise automatic file refresh, snapshot recovery and dismiss in X11."""
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
metadata_pdf = runpy.run_path(str(SCRIPTS / "smoke-properties.py"))["metadata_pdf"]


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-file-watch-") as directory:
        root = Path(directory)
        source = (SCRIPTS.parent / "assets/pagewing-guide.pdf").read_bytes()
        document = root / "pagewing-smoke.pdf"
        document.write_bytes(metadata_pdf(source, "Edition one"))
        config = root / "config/pagewing/library.json"
        config.parent.mkdir(parents=True)
        config.write_text(json.dumps({"dark": False, "sidebar": False, "recent": [{
            "path": str(document), "page": 2, "fit": {"Custom": 1.8},
            "rotation": 1, "offset": [0.35, 0.4],
        }]}))
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment,
                                           stdout=log, stderr=log)

            def book():
                try:
                    return json.loads(config.read_text())["recent"][0]
                except (OSError, ValueError, KeyError, IndexError):
                    return {}

            def details():
                keyboard.key("s"); keyboard.key("y")
                text = keyboard.clipboard()
                keyboard.key(0xFF1B)
                return text

            def view():
                state = book()
                return [state.get(name) for name in ["page", "fit", "rotation", "offset"]]

            try:
                window = wait_for(keyboard.focus, "file-watch reader window")
                assert "Title: Edition one" in wait_for(details, "initial document metadata")
                keyboard.key("m"); keyboard.key("q")
                wait_for(lambda: "q" in book().get("marks", {}), "initial view checkpoint")
                initial = view()
                assert initial[0] == 2 and initial[2] == 1 and initial[3][1] > 0
                probe = (460, 74)
                baseline = keyboard.pixel(probe)
                visible = lambda: keyboard.pixel(probe) != baseline

                # A compiler/editor can replace a PDF without changing length or mtime.
                stamp = document.stat()
                replacement = root / "replacement.pdf"
                replacement.write_bytes(metadata_pdf(source, "Edition two"))
                assert replacement.stat().st_size == stamp.st_size
                os.utime(replacement, ns=(stamp.st_atime_ns, stamp.st_mtime_ns))
                replacement.replace(document)
                wait_for(lambda: "Title: Edition two" in details(), "automatic stable atomic replacement refresh")
                # Saving a mark resolves current geometry, detecting accidental viewport changes.
                keyboard.key("m"); keyboard.key("a")
                wait_for(lambda: "a" in book().get("marks", {}), "view after automatic refresh")
                assert view() == initial, (view(), initial)
                assert not visible(), "successful automatic refresh must clear the notice"

                document.write_bytes(b"incomplete compiler output")
                wait_for(visible, "incomplete update notice")
                time.sleep(0.5)
                assert "Title: Edition two" in details(), "failed automatic reload must retain the old PDF"
                assert visible(), "failed reload must retain the file notice"
                keyboard.click((690, 88))
                wait_for(lambda: not visible(), "dismissed failed update")
                time.sleep(1.2)
                assert not visible(), "unchanged invalid file must not trigger repeated reloads"
                document.write_bytes(metadata_pdf(source, "Recovered edition"))
                wait_for(lambda: "Title: Recovered edition" in details(), "automatic recovery after a subsequent change")
                wait_for(lambda: not visible(), "successful recovery clears notice")
                keyboard.key("r", ctrl=True)
                wait_for(lambda: "Title: Recovered edition" in details(), "manual reload remains available")

                keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong,
                                                       ctypes.c_uint, ctypes.c_uint]
                keyboard.x11.XResizeWindow(keyboard.display, window, 860, 540)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.4)
                probe = (300, 74)
                baseline = keyboard.pixel(probe)
                document.write_bytes(metadata_pdf(source, "Compact edition"))
                wait_for(lambda: "Title: Compact edition" in details(), "automatic refresh in compact window")
                assert not visible(), "compact refresh must clear the notice"
                keyboard.key("w", ctrl=True)
                time.sleep(0.4)
                closed = keyboard.pixel(probe)
                document.write_bytes(source)
                time.sleep(1.3)
                assert keyboard.pixel(probe) == closed, "closed documents must stop watching"
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                print("PASS: automatic stable-change refresh, equal-size/mtime atomic replacement, "
                      "unchanged reading view, dismiss without repeats, keyboard reload, "
                      "failed reload snapshot and recovery, compact window and close")
            except Exception:
                log.flush(); log.seek(0)
                print(log.read(), file=sys.stderr)
                if config.exists():
                    print(config.read_text(), file=sys.stderr)
                raise
            finally:
                if application.poll() is None:
                    application.terminate(); application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
