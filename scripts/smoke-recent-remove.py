#!/usr/bin/env python3
"""Check recent-history removal, undo, focus, persistence and unchanged PDFs."""
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
    with tempfile.TemporaryDirectory(prefix="pagewing-recent-remove-") as directory:
        root = Path(directory)
        source = (SCRIPTS.parent / "assets/pagewing-guide.pdf").read_bytes()
        first, second, active, missing = [root / name for name in
                                          ("candidate-one.pdf", "candidate-two.pdf", "active.pdf", "missing.pdf")]
        for path in (first, second, active):
            path.write_bytes(source)
        config_path = root / "config/pagewing/library.json"
        config_path.parent.mkdir(parents=True)
        config_path.write_text(json.dumps({"recent": [
            {"path": str(path), "page": 2, "bookmarks": [1, 3],
             "marks": {"a": {"page": 4, "fit": {"Custom": 2.5}, "rotation": 1, "offset": [0.3, 0.6]}}}
            for path in (first, second, active, missing)
        ]}))
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        keyboard = Keyboard()
        keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong,
                                              ctypes.c_uint, ctypes.c_uint]
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(active)], env=environment,
                                           stdout=log, stderr=log)

            def records():
                try:
                    return {entry["path"]: entry for entry in json.loads(config_path.read_text())["recent"]}
                except (OSError, ValueError, KeyError):
                    return {}

            def text(value):
                for char in value:
                    keyboard.key(char, delay=0.02)

            def filter_to(value):
                keyboard.key("a", ctrl=True)
                text(value)

            def copied():
                keyboard.key("a", ctrl=True)
                keyboard.key("c", ctrl=True)
                return keyboard.clipboard()

            def removed(path):
                wait_for(lambda: records() and str(path) not in records(), f"remove {path.name} from saved history")

            try:
                window = wait_for(lambda: keyboard.focus("active.pdf"), "recent removal reader window")
                wait_for(lambda: records().get(str(active), {}).get("page") == 2, "active reading state")
                # Wait for normalization to the application's persisted record shape.
                wait_for(lambda: "facing" in records().get(str(first), {}), "initial full-state persistence")
                original = records()[str(first)]
                keyboard.x11.XResizeWindow(keyboard.display, window, 240, 240)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)
                keyboard.key("p", ctrl=True)
                text("candidate")
                keyboard.key(0xFF09)  # Overflowing scroll region at minimum size.
                keyboard.key(0xFF09)  # First result.
                keyboard.key(0xFFFF)  # Delete while a result owns focus.
                removed(first)
                assert copied() == "candidate", "removal retains and focuses the filter"
                # Use the Copy action; Ctrl+C on a result is not a path-copy shortcut.
                for _ in range(5):  # Undo, row, remove, Open, Copy.
                    keyboard.key(0xFF09)
                keyboard.key(0xFF0D)
                assert keyboard.clipboard() == str(second), "removal selects the nearest remaining result"
                for _ in range(4):
                    keyboard.key(0xFF09, shift=True)
                keyboard.key(0xFF0D)
                wait_for(lambda: records().get(str(first)) == original, "undo restores all saved reading state")
                assert copied() == "candidate", "undo keeps the filter text"
                for _ in range(3):  # Scroll region, first row, remove.
                    keyboard.key(0xFF09)
                keyboard.key(" ")
                removed(first)
                keyboard.key(0xFF09); keyboard.key(0xFF0D)  # Undo after automatic filter focus.
                wait_for(lambda: records().get(str(first)) == original, "Space remove button and Enter undo")
                filter_to("active")
                keyboard.key(0xFF09); keyboard.key(0xFFFF)
                keyboard.key(0xFF09); keyboard.key(0xFF09)  # Open, Copy; removal is disabled.
                keyboard.key(0xFF0D)
                assert keyboard.clipboard() == str(active), "the active row remains selected after Delete"
                assert str(active) in records() and records()[str(active)]["page"] == 2
                keyboard.key(0xFF1B)
                keyboard.x11.XResizeWindow(keyboard.display, window, 1180, 820)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)
                keyboard.key("p", ctrl=True)
                text("missing")
                keyboard.key(0xFF09); keyboard.key(0xFFFF)
                removed(missing)
                keyboard.key(0xFF1B)
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                assert str(missing) not in records()
                application = subprocess.Popen([str(binary), str(active)], env=environment,
                                               stdout=log, stderr=log)
                wait_for(lambda: keyboard.focus("active.pdf"), "restarted recent removal reader")
                keyboard.key("p", ctrl=True)
                text("missing")
                keyboard.key(0xFF0D)
                keyboard.key(0xFF1B)
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                assert str(missing) not in records(), "removed records must stay removed after restart"
                assert records()[str(first)] == original
                assert not missing.exists()
                for path in (first, second, active):
                    assert path.read_bytes() == source, "recent-history edits must never modify PDF files"
                print("PASS: recent row Delete and Space remove, minimum-size undo, complete saved-state restoration, retained query and nearest selection, active-document guard, missing-file removal, persisted restart, unchanged PDFs")
            except Exception:
                if application.poll() is None:
                    print(f"Failure process status: {application.poll()!r}", file=sys.stderr)
                    try:
                        print(f"Failure focused text or path: {copied()!r}", file=sys.stderr)
                    except Exception as error:
                        print(f"Failure focus diagnostic unavailable: {error}", file=sys.stderr)
                log.flush(); log.seek(0)
                print(log.read(), file=sys.stderr)
                if config_path.exists():
                    print(config_path.read_text(), file=sys.stderr)
                raise
            finally:
                if application.poll() is None:
                    application.terminate(); application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
