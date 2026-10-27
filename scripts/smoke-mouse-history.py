#!/usr/bin/env python3
"""Exercise real mouse side buttons, full view history and modal input guards."""
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
    with tempfile.TemporaryDirectory(prefix="pagewing-mouse-history-") as directory:
        root = Path(directory)
        document = root / "pagewing-smoke.pdf"
        document.write_bytes((SCRIPTS.parent / "assets/pagewing-guide.pdf").read_bytes())
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/pagewing/library.json"
        keyboard = Keyboard()
        keyboard.xtst.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
        keyboard.xtst.XTestFakeButtonEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)

            def book():
                try:
                    return json.loads(config_path.read_text())["recent"][0]
                except (OSError, ValueError, KeyError, IndexError):
                    return {}

            def expect(predicate, description):
                def saved():
                    item = book()
                    return item if item and predicate(item) else None
                return wait_for(saved, description)

            def sequence(text, burst=False):
                if burst:
                    application.send_signal(signal.SIGSTOP)
                try:
                    for character in text:
                        keyboard.key(character.lower(), shift=character.isupper() or character == "+", delay=0 if burst else 0.025)
                finally:
                    if burst:
                        application.send_signal(signal.SIGCONT)

            def button(forward=False, delay=0.15):
                keyboard.xtst.XTestFakeMotionEvent(keyboard.display, -1, 800, 500, 0)
                for pressed in (1, 0):
                    keyboard.xtst.XTestFakeButtonEvent(keyboard.display, 9 if forward else 8, pressed, 0)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(delay)

            def same_view(actual, expected):
                return all(actual.get(key) == expected[key] for key in ("page", "fit", "rotation")) and all(abs(a - b) < 0.001 for a, b in zip(actual["offset"], expected["offset"]))

            def restored(view, description):
                expect(lambda item: same_view(item, view), description)

            try:
                wait_for(keyboard.focus, "mouse history reader window")
                expect(lambda item: item["page"] == 0, "initial document")
                sequence("wzb")
                first = expect(lambda item: item["fit"] == "Width" and item["offset"][1] == 1, "first full reading view")
                sequence("Jr+zz")
                second = expect(lambda item: item["page"] == 1 and item["rotation"] == 1 and isinstance(item["fit"], dict) and abs(item["offset"][1] - 0.5) < 0.001, "second rotated and zoomed reading view")
                sequence("''", burst=True)
                restored(first, "Vim quotes restore the full view before a jump")
                sequence("``", burst=True)
                restored(second, "Vim backticks toggle to the view just left")
                sequence("''``", burst=True)
                restored(second, "two same-frame Vim last-jump commands preserve their order")
                button()
                restored(first, "mouse Back restores page, fit, rotation and scroll")
                button(True)
                restored(second, "mouse Forward restores the later full view")
                application.send_signal(signal.SIGSTOP)
                try:
                    button(delay=0); button(True, delay=0); button(delay=0)
                finally:
                    application.send_signal(signal.SIGCONT)
                restored(first, "multiple side buttons in one frame preserve event order")
                button(True)
                restored(second, "forward remains valid after a same-frame round trip")
                keyboard.key("s"); button()
                keyboard.key("y")
                assert "Current page: ii · 2" in keyboard.clipboard(), "information window ignores side-button navigation"
                keyboard.key(0xFF1B)
                keyboard.key("/")
                sequence("jkgquery")
                button()
                keyboard.key("a", ctrl=True); keyboard.key("c", ctrl=True)
                copied = keyboard.clipboard()
                assert copied == "jkgquery", ("side buttons preserve search input focus and text", copied)
                expect(lambda item: all(item.get(key) == second[key] for key in ("page", "fit", "rotation")), "side buttons do not navigate during text input")
                keyboard.key(0xFF1B)
                keyboard.key("p", ctrl=True)
                sequence("pagewing")
                button()
                keyboard.key("a", ctrl=True); keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == "pagewing", "recent picker ignores side buttons"
                keyboard.key(0xFF1B)
                button()
                restored(first, "input guards leave history available after Escape")
                sequence("99m")
                application.send_signal(signal.SIGSTOP)
                try:
                    button(True, delay=0)
                    keyboard.key("j", shift=True, delay=0)
                finally:
                    application.send_signal(signal.SIGCONT)
                expect(lambda item: item["page"] == 2 and not item["marks"], "side buttons consume counts and cancel mark prompts before later same-frame navigation")
                keyboard.key("w", ctrl=True)
                button(); button(True)
                keyboard.key("p", ctrl=True)
                sequence("pagewing-smoke")
                keyboard.key(0xFF0D)
                expect(lambda item: item["page"] == 2, "reopen saved view after side buttons on welcome")
                time.sleep(0.4)
                sequence("''", burst=True)
                expect(lambda item: item["page"] == 2, "reopened documents start with no last-jump position")
                sequence("999")
                button()
                sequence("J")
                expect(lambda item: item["page"] == 3, "empty history still consumes the numeric prefix")
                sequence("/''``")
                keyboard.key("a", ctrl=True); keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == "''``", "last-jump keys remain literal in search inputs"
                keyboard.key(0xFF1B)
                sequence("Rwzbma")
                same_page = expect(lambda item: item["page"] == 3 and item["fit"] == "Width"
                                   and item["offset"][1] == 1, "save bottom mark for a same-page jump")
                sequence("p")
                fitted = expect(lambda item: item["page"] == 3 and item["fit"] == "Page", "fit same page before restoring a mark")
                sequence("'a", burst=True)
                restored(same_page, "named mark still restores the saved full view")
                sequence("''", burst=True)
                restored(fitted, "Vim quotes return to the previous view on the same page")
                sequence("``", burst=True)
                restored(same_page, "Vim backticks restore same-page scrolling and fit")
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                print("PASS: real mouse Back/Forward, Vim last-jump toggles, full and same-page reading views, ordered same-frame round trips, modal/search/recent guards, input focus, counts and mark prompts, welcome and empty history")
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
