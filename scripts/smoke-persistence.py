#!/usr/bin/env python3
"""Check background persistence and graceful quit. Run with xvfb-run -a."""
import json
import os
from pathlib import Path
import runpy
import shutil
import subprocess
import sys
import tempfile
import time

SCRIPTS = Path(__file__).resolve().parent
desktop = runpy.run_path(str(SCRIPTS / "smoke-desktop.py"))
Keyboard, wait_for = desktop["Keyboard"], desktop["wait_for"]


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-persistence-") as directory:
        root = Path(directory)
        document = root / "pagewing-smoke.pdf"
        shutil.copyfile(SCRIPTS.parent / "assets/pagewing-guide.pdf", document)
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/pagewing/library.json"
        keyboard = Keyboard()
        application = None
        with open(root / "application.log", "w+") as log:
            def launch():
                process = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)
                wait_for(keyboard.focus, "persistence reader window")
                return process

            def saved(predicate):
                try:
                    state = json.loads(config_path.read_text())
                    return predicate(state, state["recent"][0])
                except (OSError, ValueError, KeyError, IndexError):
                    return False

            def quit():
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0, "Reader did not exit cleanly"

            try:
                application = launch()
                wait_for(lambda: saved(lambda _, book: book["page"] == 0), "initial background save")
                # Quit well before the next two-second autosave: on-exit must flush.
                keyboard.key("5"); keyboard.key("g", shift=True)
                keyboard.key("b"); keyboard.key("r"); keyboard.key("t")
                quit()
                assert saved(lambda state, book: book["page"] == 4 and book["rotation"] == 1 and 4 in book["bookmarks"] and state["thumbnails"] and state["sidebar"])
                application = launch()
                time.sleep(0.3)
                keyboard.key("2"); keyboard.key("g", shift=True)
                quit()
                assert saved(lambda _, book: book["page"] == 1 and book["rotation"] == 1 and 4 in book["bookmarks"])
                # A genuine write failure must leave the reader usable and retry.
                config_path.unlink()
                (config_path.parent / "config.toml").unlink()
                (config_path.parent / "config.schema.json").unlink()
                config_path.parent.rmdir()
                config_path.parent.write_text("a file blocks the settings directory")
                application = launch()
                time.sleep(2.3)
                keyboard.key("3"); keyboard.key("g", shift=True)
                keyboard.key("d")
                config_path.parent.unlink()
                wait_for(lambda: saved(lambda state, book: book["page"] == 2 and state["dark"]), "retry saves current state after fixing the path")
                quit()
                assert saved(lambda state, book: book["page"] == 2 and state["dark"])
                print("PASS: background autosave, immediate graceful quit, latest page/bookmark/rotation/preferences, restart and real filesystem error recovery")
            except Exception:
                log.flush(); log.seek(0)
                print(log.read(), file=sys.stderr)
                if config_path.exists():
                    print(config_path.read_text(), file=sys.stderr)
                raise
            finally:
                if application and application.poll() is None:
                    application.terminate(); application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
