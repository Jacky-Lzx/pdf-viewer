#!/usr/bin/env python3
"""Queue a native mouse click and typing while stopped, then check input routing."""
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
    with tempfile.TemporaryDirectory(prefix="pagewing-mouse-input-") as directory:
        root = Path(directory)
        document = root / "pagewing-smoke.pdf"
        document.write_bytes((SCRIPTS.parent / "assets/pagewing-guide.pdf").read_bytes())
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/pagewing/library.json"
        config_path.parent.mkdir(parents=True)
        config_path.write_text('{"sidebar":true}')
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)

            def expect(page):
                def saved():
                    try:
                        config = json.loads(config_path.read_text())
                        book = config["recent"][0]
                        return (book["page"] == page and book["rotation"] == 0
                                and book["fit"] == "Page" and not book["marks"] and not book["bookmarks"]
                                and not any(config[name] for name in ("dark", "night", "thumbnails")))
                    except (OSError, ValueError, KeyError, IndexError):
                        return False
                wait_for(saved, f"reading page {page + 1} without input leaking into reader actions")

            def click_burst(position, value, enter=False, edit=False):
                application.send_signal(signal.SIGSTOP)
                try:
                    keyboard.click(position)
                    keyboard.key("a", ctrl=True, delay=0)
                    for letter in value:
                        keyboard.key(letter, delay=0)
                    if edit:
                        keyboard.key(0xFF51, delay=0)
                        keyboard.key("q", delay=0)
                    if enter:
                        keyboard.key(0xFF0D, delay=0)
                finally:
                    application.send_signal(signal.SIGCONT)
                time.sleep(0.4)

            def copied_query(value):
                keyboard.key("a", ctrl=True)
                keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == value, (keyboard.clipboard(), value)

            try:
                wait_for(keyboard.focus, "mouse input reader window")
                expect(0)
                keyboard.key("m", ctrl=True, shift=True)  # Use the full reading canvas.
                keyboard.key("y")
                assert "native PDF reader" in keyboard.clipboard()
                click_burst((90, 171), "jkgdimouse")
                copied_query("jkgdimouse")
                expect(0)
                keyboard.key(0xFF1B)
                keyboard.key("j", shift=True)
                expect(1)
                keyboard.key("/")
                time.sleep(0.3)
                keyboard.click((800, 500))
                click_burst((150, 77), "jkgdisearch", edit=True)
                copied_query("jkgdisearcqh")
                expect(1)
                keyboard.click((800, 500))
                click_burst((150, 77), "native", enter=True)
                expect(0)
                keyboard.key("f", ctrl=True)
                copied_query("native")
                keyboard.key(0xFF1B)
                keyboard.key("j", shift=True)
                expect(1)
                # Opening a toolbar input and typing can arrive in one busy frame.
                click_burst((1050, 28), "jkgditoolbar")
                copied_query("jkgditoolbar")
                expect(1)
                keyboard.key(0xFF1B)
                keyboard.key("9"); keyboard.key("9"); keyboard.key("m")
                click_burst((130, 805), "4", enter=True)
                expect(3)
                print("PASS: same-frame mouse click plus chapter/search text, toolbar search and bottom page input, actual clipboard content, immediate Enter, no reader action leakage and Escape focus release")
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
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
