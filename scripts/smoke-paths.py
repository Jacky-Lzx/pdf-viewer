#!/usr/bin/env python3
"""Check raw Unix PDF paths through CLI, persistence and recent-document reopen."""
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
    with tempfile.TemporaryDirectory(prefix="pagewing-paths-") as directory:
        root = Path(directory)
        # These directories have identical lossy display text but distinct bytes.
        paths = [root / os.fsdecode(b"docs-\xff") / "pagewing-smoke.pdf",
                 root / os.fsdecode(b"docs-\xfe") / "pagewing-smoke.pdf",
                 root / "ordinary.pdf"]
        for path in paths:
            path.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(SCRIPTS.parent / "assets/pagewing-guide.pdf", path)
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/pagewing/library.json"
        keyboard = Keyboard()
        application = None
        with open(root / "application.log", "w+") as log:
            def launch(path=None):
                process = subprocess.Popen([str(binary)] + ([str(path)] if path else []),
                                           env=environment, stdout=log, stderr=log)
                wait_for(lambda: keyboard.focus(path.name if path else "Pagewing"), "path reader window")
                return process

            def saved(path, page, rotation, mark=None):
                try:
                    book = json.loads(config_path.read_text())["recent"][0]
                    encoded = book["path"]
                    actual = bytes(encoded["unix_bytes"]) if isinstance(encoded, dict) else os.fsencode(encoded)
                    return (actual == os.fsencode(path) and book["page"] == page
                            and book["rotation"] == rotation
                            and (not mark or book["marks"][mark]["page"] == (4 if mark == "a" else 1)))
                except (OSError, ValueError, KeyError, IndexError):
                    return False

            def quit():
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0, "Reader did not exit cleanly"

            def reopen_closed(path, page, rotation, mark):
                keyboard.key("w", ctrl=True)
                keyboard.key("t", ctrl=True, shift=True)
                time.sleep(0.8)
                # A changed page proves the raw path actually loaded again;
                # the unchanged saved state alone could be stale after close.
                keyboard.key("j", shift=True)
                wait_for(lambda: saved(path, page + 1, rotation, mark), "closed raw path reloads and retains its rotation/mark")

            try:
                application = launch(paths[0])
                wait_for(lambda: saved(paths[0], 0, 0), "initial non-UTF-8 path save")
                for key, shift in [("5", False), ("g", True), ("r", False),
                                   ("b", False), ("m", False), ("a", False),
                                   ("3", False), ("g", True)]:
                    keyboard.key(key, shift=shift)
                quit()
                assert saved(paths[0], 2, 1, "a")

                application = launch(paths[1])
                wait_for(lambda: saved(paths[1], 0, 0), "distinct raw path starts at its own page")
                for key, shift in [("2", False), ("g", True), ("3", False),
                                   ("r", False), ("b", False), ("m", False),
                                   ("b", False), ("6", False), ("g", True)]:
                    keyboard.key(key, shift=shift)
                quit()
                assert saved(paths[1], 5, 3, "b")

                # Saving an ordinary path must retain both raw paths too.
                application = launch(paths[2])
                wait_for(lambda: saved(paths[2], 0, 0), "ordinary path joins the same library")
                quit()
                books = json.loads(config_path.read_text())["recent"]
                assert len(books) == 3 and isinstance(books[0]["path"], str)
                assert books[1]["bookmarks"] == [1] and books[2]["bookmarks"] == [4]

                application = launch()
                keyboard.key("p", ctrl=True)
                keyboard.key(0xFF54); keyboard.key(0xFF0D)
                wait_for(lambda: saved(paths[1], 5, 3, "b"), "recent picker restores second raw path")
                keyboard.key("'"); keyboard.key("b")
                wait_for(lambda: saved(paths[1], 1, 3, "b"), "restored raw-path named mark")
                reopen_closed(paths[1], 1, 3, "b")
                keyboard.key("'"); keyboard.key("b")
                wait_for(lambda: saved(paths[1], 1, 3, "b"), "mark survives the second raw path's shortcut reopen")
                keyboard.key("p", ctrl=True)
                keyboard.key("j", ctrl=True); keyboard.key("j", ctrl=True); keyboard.key(0xFF0D)
                wait_for(lambda: saved(paths[0], 2, 1, "a"), "recent picker restores first raw path")
                reopen_closed(paths[0], 2, 1, "a")
                keyboard.key("'"); keyboard.key("a")
                quit()
                assert saved(paths[0], 4, 1, "a")
                print("PASS: distinct non-UTF-8 Unix paths, ordinary paths in the same library, "
                      "background save, immediate quit, restart, recent and closed-document shortcut reopen, retained rotation, bookmarks and named marks")
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
