#!/usr/bin/env python3
"""Close native documents, preserve progress, and reopen without stale UI/jobs."""
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


def run(binary):
    with tempfile.TemporaryDirectory(prefix="folio-close-") as directory:
        root = Path(directory)
        document = root / "folio-smoke.pdf"
        document.write_bytes((SCRIPTS.parent / "assets/folio-guide.pdf").read_bytes())
        encrypted = root / "encrypted.pdf"
        encrypted.write_bytes((SCRIPTS.parent / "tests/fixtures/encrypted.pdf").read_bytes())
        missing = root / "missing.pdf"
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/folio/library.json"
        config_path.parent.mkdir(parents=True)
        config_path.write_text(json.dumps({"sidebar": True, "recent": [
            {"path": str(encrypted), "page": 1}, {"path": str(missing), "page": 0}
        ]}))
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)
            window = None

            def title():
                name = ctypes.c_char_p()
                if keyboard.x11.XFetchName(keyboard.display, window, ctypes.byref(name)) and name.value:
                    try:
                        return name.value.decode("utf-8")
                    finally:
                        keyboard.x11.XFree(name)
                return ""

            def book():
                try:
                    return next(item for item in json.loads(config_path.read_text())["recent"] if item["path"] == str(document))
                except (OSError, ValueError, KeyError, StopIteration):
                    return {}

            def recent(query):
                keyboard.key("p", ctrl=True, delay=0)
                for letter in query:
                    keyboard.key(letter, delay=0)
                keyboard.key(0xFF0D, delay=0)

            def closed():
                wait_for(lambda: title() == "Folio", "closing returns to welcome in the same window")
                assert application.poll() is None

            def reopened():
                wait_for(lambda: title().startswith("folio-smoke.pdf"), "recent document reopens")
                time.sleep(0.5)
                keyboard.key("y")
                assert "Make yourself at home." in keyboard.clipboard(), "the saved page and actual PDF must be restored"

            try:
                window = wait_for(keyboard.focus, "reader window")
                wait_for(lambda: book().get("page") == 0, "initial document saved")
                keyboard.key("3"); keyboard.key("g", shift=True)
                keyboard.key("w"); keyboard.key("z"); keyboard.key("b")
                keyboard.key("m"); keyboard.key("a"); keyboard.key("b")
                wait_for(lambda: book().get("page") == 2 and book().get("offset", [0, 0])[1] > 0.99, "width fit and bottom position")
                keyboard.key("/")
                for letter in "jkg":
                    keyboard.key(letter)
                wait_for(lambda: 0.8 < book().get("offset", [0, 0])[1] < 0.99, "search viewport updates the saved relative position")
                before_close = book()
                keyboard.key("w", ctrl=True)
                closed()
                saved = book()
                assert saved["page"] == 2 and saved["fit"] == "Width"
                assert abs(saved["offset"][1] - before_close["offset"][1]) < 0.0001
                assert 2 in saved["bookmarks"] and "a" in saved["marks"]
                keyboard.key("w", ctrl=True)
                keyboard.key("j", shift=True)
                assert book() == saved, "closing welcome and reader keys cannot overwrite saved progress"
                recent("folio-smoke")
                reopened()
                assert book()["marks"] == saved["marks"]
                # Accumulate close, picker, query and Enter into the same frame.
                application.send_signal(signal.SIGSTOP)
                try:
                    keyboard.key("w", ctrl=True, delay=0)
                    recent("folio-smoke")
                finally:
                    application.send_signal(signal.SIGCONT)
                time.sleep(0.8)
                reopened()
                keyboard.key("w", ctrl=True)
                closed()
                recent("encrypted")
                wait_for(lambda: title().startswith("encrypted.pdf"), "encrypted document selected")
                time.sleep(0.5)
                keyboard.key("w", ctrl=True)
                closed()
                recent("folio-smoke")
                reopened()
                keyboard.key("w", ctrl=True)
                closed()
                recent("missing")
                wait_for(lambda: title().startswith("missing.pdf"), "missing document selected")
                time.sleep(0.5)
                keyboard.key("w", ctrl=True)
                closed()
                recent("folio-smoke")
                reopened()
                keyboard.click((210, 28))
                closed()
                recent("folio-smoke")
                reopened()
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                assert book()["page"] == 2 and abs(book()["offset"][1] - saved["offset"][1]) < 0.0001
                print("PASS: close from search, immediate progress save, same-window welcome, repeated close, recent reopen, same-frame close/reopen, password and missing-file dialogs, toolbar button, marks/bookmarks and graceful quit")
            except Exception:
                log.flush(); log.seek(0)
                print(log.read(), file=sys.stderr)
                if config_path.exists():
                    print(config_path.read_text(), file=sys.stderr)
                raise
            finally:
                if application.poll() is None:
                    application.send_signal(signal.SIGCONT)
                    application.terminate(); application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/folio").resolve())
