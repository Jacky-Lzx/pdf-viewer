#!/usr/bin/env python3
"""Exercise relative page input, view/history preservation and signed PDF labels."""
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
    with tempfile.TemporaryDirectory(prefix="pagewing-relative-") as directory:
        root = Path(directory)
        document = root / "pagewing-smoke.pdf"
        source = (SCRIPTS.parent / "assets/pagewing-guide.pdf").read_bytes()
        document.write_bytes(source)
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/pagewing/library.json"
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)

            def book():
                try:
                    return json.loads(config_path.read_text())["recent"][0]
                except (OSError, ValueError, KeyError, IndexError):
                    return {}

            def expect(page, vertical=None, fit=None, rotation=0):
                def saved():
                    item = book()
                    return item if item.get("page") == page and item.get("rotation") == rotation and (
                        vertical is None or abs(item["offset"][1] - vertical) < 0.001
                    ) and (fit is None or item["fit"] == fit) else None
                return wait_for(saved, f"relative page {page + 1}, offset {vertical}, fit {fit}")

            def sequence(value, delay=0.025):
                for character in value:
                    keyboard.key(character.lower(), shift=character.isupper() or character in "+:", delay=delay)

            def goto(value):
                application.send_signal(signal.SIGSTOP)
                try:
                    sequence(":" + value, delay=0)
                    keyboard.key(0xFF0D, delay=0)
                finally:
                    application.send_signal(signal.SIGCONT)

            try:
                wait_for(keyboard.focus, "relative page reader window")
                expect(0, fit="Page")
                keyboard.key("m", ctrl=True, shift=True)  # Use the full reading canvas.
                for query, page in [("+2", 2), ("-1", 1), ("A-2", 5), ("-3", 2),
                                    ("2", 1), ("+9999999", 5), ("-9999999", 0)]:
                    goto(query)
                    expect(page, fit="Page")
                sequence("Jw+++zb")
                original = expect(1, 1)
                goto("+2")
                expect(3, 0, original["fit"])
                keyboard.key(0xFF51, alt=True)
                expect(1, 1, original["fit"])
                goto("+0")
                # If a zero offset re-records history, Forward would disappear.
                time.sleep(0.25)
                expect(1, 1, original["fit"])
                keyboard.key(0xFF53, alt=True)
                expect(3, 0, original["fit"])
                sequence("zb")
                expect(3, 1, original["fit"])
                goto("-0")
                time.sleep(0.25)
                expect(3, 1, original["fit"])
                sequence("999")
                goto("-1")
                expect(2, 0, original["fit"])
                sequence("J")
                expect(3, 0, original["fit"])
                goto("+-2")
                time.sleep(0.3)
                keyboard.key("a", ctrl=True); keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == "+-2", "invalid relative input stays editable"
                sequence("-1")
                keyboard.key(0xFF0D)
                expect(2, 0, original["fit"])
                goto("+1")
                expect(3, 0, original["fit"])
                sequence("/+2 -3")
                keyboard.key("a", ctrl=True); keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == "+2 -3", "relative notation stays literal in search"
                keyboard.key(0xFF1B)
                sequence("r")
                expect(3, fit=original["fit"], rotation=1)
                goto("-1")
                expect(2, fit=original["fit"], rotation=1)
                keyboard.key(0xFF51, alt=True)
                expect(3, fit=original["fit"], rotation=1)
                sequence("R")
                expect(3, fit=original["fit"])
                sequence("ggp")
                expect(0, fit="Page")
                for prefix, label in [(b"+2", "=+21"), (b"-2", "=-21")]:
                    document.write_bytes(source.replace(b"/P (A-)", b"/P (" + prefix + b")", 1))
                    keyboard.key("r", ctrl=True)
                    time.sleep(0.5)
                    goto(label)
                    expect(4, fit="Page")
                    goto("-1")
                    expect(3, fit="Page")
                goto("=1")
                expect(2, fit="Page")
                goto("1")
                expect(0, fit="Page")
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                print("PASS: relative page offsets, same-frame dialog input/Enter, edge clamping, retained zoom/rotation/scroll/history, zero-offset Forward, count cleanup, invalid input, search isolation and explicit signed/numeric PDF labels")
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
