#!/usr/bin/env python3
"""Queue native key bursts while stopped, then check input focus and navigation."""
import json
import os
from pathlib import Path
import runpy
import shutil
import signal
import subprocess
import sys
import tempfile
import time

SCRIPTS = Path(__file__).resolve().parent
desktop = runpy.run_path(str(SCRIPTS / "smoke-desktop.py"))
Keyboard, wait_for = desktop["Keyboard"], desktop["wait_for"]
ENTER, ESCAPE = 0xFF0D, 0xFF1B


def text(value):
    return [(character.lower(), {"shift": character.isupper()}) for character in value]


def run(binary):
    with tempfile.TemporaryDirectory(prefix="folio-input-burst-") as directory:
        root = Path(directory)
        paths = [root / name for name in ("folio-smoke.pdf", "plan-a.pdf", "plan-z.pdf")]
        for path in paths:
            shutil.copyfile(SCRIPTS.parent / "assets/folio-guide.pdf", path)
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/folio/library.json"
        config_path.parent.mkdir(parents=True)
        config_path.write_text(json.dumps({"recent": [{"path": str(path), "page": index * 2} for index, path in enumerate(paths)]}))
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(paths[0])], env=environment, stdout=log, stderr=log)

            def expect(path, page, description):
                def saved():
                    try:
                        config = json.loads(config_path.read_text())
                        book = config["recent"][0]
                        return (book["path"] == str(path) and book["page"] == page
                                and book["rotation"] == 0 and book["fit"] == "Page"
                                and not any(config[name] for name in ("dark", "night", "thumbnails")))
                    except (OSError, ValueError, KeyError, IndexError):
                        return False
                wait_for(saved, description)

            def burst(keys):
                # This exercises a busy application, without relying on how
                # quickly the machine happens to render consecutive key events.
                application.send_signal(signal.SIGSTOP)
                try:
                    for symbol, modifiers in keys:
                        keyboard.key(symbol, delay=0, **modifiers)
                finally:
                    application.send_signal(signal.SIGCONT)
                time.sleep(0.4)

            def copy_query(expected):
                keyboard.key("a", ctrl=True)
                keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == expected, (keyboard.clipboard(), expected)

            try:
                wait_for(keyboard.focus, "burst-input reader window")
                expect(paths[0], 0, "initial reading state")
                # Seed a nonempty clipboard so a lost query cannot pass as stale
                # clipboard content or hang waiting for an empty selection.
                keyboard.key("y")
                assert "native PDF reader" in keyboard.clipboard()
                burst([("/", {})] + text("jkgdi/path"))
                copy_query("jkgdi/path")
                keyboard.key(ESCAPE)
                keyboard.key("j", shift=True)
                expect(paths[0], 1, "search typing neither navigates nor changes reading preferences")

                burst([("f", {"ctrl": True}), ("a", {"ctrl": True})] + text("native") + [(ENTER, {})])
                expect(paths[0], 0, "opening search, replacing text and Enter in one burst finds the right page")
                keyboard.key("f", ctrl=True)
                copy_query("native")
                keyboard.key(ESCAPE)

                burst([(':', {"shift": True})] + text("2") + [(ENTER, {})])
                expect(paths[0], 1, "return to a page between Rust matches")
                burst([("f", {"ctrl": True}), ("a", {"ctrl": True})] + text("Rust") + [(ENTER, {"shift": True})])
                expect(paths[0], 0, "Shift+Enter uses the key's modifiers even when Shift is released in the same frame")
                keyboard.key(ESCAPE)

                burst([(':', {"shift": True})] + text("4") + [(ENTER, {})])
                expect(paths[0], 3, "physical page dialog accepts its opening-frame text and Enter")
                burst([(':', {"shift": True})] + text("A-2") + [(ENTER, {})])
                expect(paths[0], 5, "page labels retain uppercase letters without inserting the colon")

                burst([("p", {"ctrl": True})] + text("plan-z") + [(ENTER, {})])
                expect(paths[2], 4, "recent picker filters and opens the latest text in its first frame")
                burst([("p", {"ctrl": True})] + text("plan-a"))
                copy_query("plan-a")
                keyboard.key(ESCAPE)
                keyboard.key("j", shift=True)
                expect(paths[2], 5, "cancelled recent input restores normal navigation")

                # A second global shortcut can replace a dialog in the same burst.
                burst([(':', {"shift": True}), ("f", {"ctrl": True}), ("a", {"ctrl": True})] + text("native") + [(ENTER, {})])
                expect(paths[2], 0, "search replaces page dialog and receives the remaining keys")
                keyboard.key(ESCAPE)
                burst([("/", {}), (ESCAPE, {}), ("j", {"shift": True})])
                expect(paths[2], 1, "Escape cancels pending input focus before the next navigation key")
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                print("PASS: same-frame search/replacement/Enter, no navigation or preference leakage, page numbers/labels, recent filtering/open, dialog switching and Escape focus release")
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
