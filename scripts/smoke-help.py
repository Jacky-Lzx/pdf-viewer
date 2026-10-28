#!/usr/bin/env python3
"""Exercise filtered shortcut help, clipboard, focus and reader isolation."""
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
TAB, ENTER, ESCAPE = 0xFF09, 0xFF0D, 0xFF1B


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-help-") as directory:
        root = Path(directory)
        document = root / "pagewing-smoke.pdf"
        document.write_bytes((SCRIPTS.parent / "assets/pagewing-guide.pdf").read_bytes())
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/pagewing/library.json"
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)

            def expect(page):
                def saved():
                    try:
                        config = json.loads(config_path.read_text())
                        book = config["recent"][0]
                        return (book["page"] == page and book["fit"] == "Page" and book["rotation"] == 0
                                and not book["bookmarks"] and not book["marks"]
                                and not config["dark"] and not config["night"])
                    except (OSError, ValueError, KeyError, IndexError):
                        return False
                wait_for(saved, f"reader state on page {page + 1}")

            def text(value, delay=0.025):
                for character in value:
                    keyboard.key(character.lower(), shift=character.isupper() or character == "?", delay=delay)

            def burst(value, toolbar=False):
                if toolbar:
                    keyboard.wheel((1100, 28), down=True, steps=20)
                application.send_signal(signal.SIGSTOP)
                try:
                    if toolbar:
                        keyboard.click((1140, 28))
                    text(value, delay=0)
                finally:
                    application.send_signal(signal.SIGCONT)
                time.sleep(0.4)

            def query(value):
                keyboard.key("a", ctrl=True)
                keyboard.key(0xFF08)
                text(value)

            def copy_matches(required, excluded=(), tabs=1):
                for _ in range(tabs):
                    keyboard.key(TAB)
                keyboard.key(ENTER)
                def copied():
                    result = keyboard.clipboard()
                    return result if result.startswith("Pagewing keyboard shortcuts\n\n") and all(row in result for row in required) and all(row not in result for row in excluded) else None
                try:
                    return wait_for(copied, "filtered shortcut rows copied with Tab/Enter")
                except AssertionError:
                    print("Clipboard after copy:", repr(keyboard.clipboard()), file=sys.stderr)
                    raise

            try:
                window = wait_for(keyboard.focus, "shortcut help reader window")
                expect(0)
                burst("?zoom")
                keyboard.key("a", ctrl=True); keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == "zoom", "same-frame help opening must focus its filter"
                copy_matches(["Zoom in / out", "100% zoom"], ["Toggle bookmark", "Copy page text"])
                query("COPY LINK")
                copied = copy_matches(["Copy a visible web-link address", "Right-click web link"], ["Copy page text", "Zoom in"])
                assert len(copied.splitlines()) == 4, copied
                query("")
                # The scroll content and vertical scroll bar precede the copy button.
                copy_matches(["Zoom in / out", "Toggle bookmark", "Copy page text", "Go to page dialog", "PageUp/PageDown", "chapter or recent filter"], tabs=3)
                query("jkgdi missing")
                keyboard.key("a", ctrl=True); keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == "jkgdi missing"
                keyboard.key(ESCAPE); keyboard.key("j", shift=True)
                expect(1)

                keyboard.key(0xFFBE)  # F1
                text("rotate")
                keyboard.key("a", ctrl=True); keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == "rotate", "reopening help clears the old query"
                copy_matches(["Rotate clockwise / counterclockwise"], ["Zoom in", "Toggle bookmark"])
                keyboard.key("f", ctrl=True)
                text("jkgdi")
                burst("bookmark", toolbar=True)
                keyboard.key("a", ctrl=True); keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == "bookmark", f"toolbar help takes focus from search before batched typing: {keyboard.clipboard()!r}"
                copy_matches(["Toggle bookmark", "Next / previous bookmark"], ["Zoom in"])
                keyboard.key(ESCAPE)
                text("99m")
                keyboard.wheel((1100, 28), down=True, steps=20)
                keyboard.click((1140, 28))
                keyboard.key(ESCAPE); keyboard.key("j", shift=True)
                expect(2)

                keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_uint, ctypes.c_uint]
                keyboard.x11.XResizeWindow(keyboard.display, window, 860, 540)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)
                burst("?copy link")
                copy_matches(["Copy a visible web-link address", "Right-click web link"])
                keyboard.key(ESCAPE)

                keyboard.x11.XResizeWindow(keyboard.display, window, 240, 240)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)
                burst("?copy link")
                # Stacked results scroll; both actions remain below the results.
                copy_matches(["Copy a visible web-link address", "Right-click web link"], tabs=3)
                # Copy returns focus to the filter; Tab through the scroller,
                # scrollbar and Copy to activate Close with the keyboard.
                for _ in range(4):
                    keyboard.key(TAB)
                keyboard.key(ENTER)
                keyboard.key("k", shift=True)
                expect(1)

                burst("?missing action")
                # Empty results disable Copy, leaving Close directly after input.
                keyboard.key(TAB); keyboard.key(ENTER)
                keyboard.key("k", shift=True)
                expect(0)
                keyboard.key("w", ctrl=True)
                time.sleep(0.3)
                burst("?zoom")
                copy_matches(["Zoom in / out", "100% zoom"], tabs=3)
                keyboard.key(ESCAPE)
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                (config_path.parent / "config.toml").write_text("[shortcuts]\nnext_page = ['x']\n")
                application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)
                window = wait_for(keyboard.focus, "reader with remapped page command")
                expect(0)
                burst("?next page")
                copy_matches(["X\tNext page"], ["J / K"])
                keyboard.key(ESCAPE)
                keyboard.key("x")
                expect(1)
                burst("?next page")
                copy_matches(["X\tNext page"], ["J / K"])
                keyboard.key(ESCAPE)
                keyboard.x11.XResizeWindow(keyboard.display, window, 1180, 820)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)
                burst("?pageup")
                copy_matches(["PageUp/PageDown", "chapter or recent filter"])
                keyboard.key(ESCAPE)
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                print("PASS: same-frame help filter, case-insensitive multi-token matching, copied visible/all shortcuts, empty matches, F1/reset, search/toolbar focus, count/mark cleanup, compact and minimum-size copy/close actions, welcome screen and reopened help with a functioning remapped page command")
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
