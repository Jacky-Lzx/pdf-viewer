#!/usr/bin/env python3
"""Copy real PDF web links through a native context menu, without opening them."""
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
URL = "https://www.rust-lang.org/"


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-link-menu-") as directory:
        root = Path(directory)
        document = root / "pagewing-smoke.pdf"
        document.write_bytes((SCRIPTS.parent / "assets/pagewing-guide.pdf").read_bytes())
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/pagewing/library.json"
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)

            def book():
                try:
                    return json.loads(config_path.read_text())["recent"][0]
                except (OSError, ValueError, KeyError, IndexError):
                    return {}

            def expect(page, rotation=0):
                wait_for(lambda: book().get("page") == page and book().get("rotation") == rotation, f"page {page + 1}, rotation {rotation}")

            def point(width=1180, height=820, rotation=0, search=False):
                # Guide URI annotation center in the rendered page's coordinates.
                x, y = (164, 417.5) if rotation else (417.5, 636)
                pw, ph = (800, 600) if rotation else (600, 800)
                vw, vh = width - 48, height - 133 - (42 if search else 0)
                scale = min(vw / pw, vh / ph)
                return (round(24 + (vw - pw * scale) / 2 + x * scale),
                        round(80 + (42 if search else 0) + (vh - ph * scale) / 2 + y * scale))

            def menu(position):
                keyboard.click(position, button=3)
                time.sleep(0.15)

            def copy_menu(position):
                menu(position)
                keyboard.click((position[0] + 50, position[1] + 16))
                assert keyboard.clipboard() == URL

            try:
                window = wait_for(keyboard.focus, "link menu reader window")
                expect(0)
                keyboard.key("y")
                assert "native PDF reader" in keyboard.clipboard()
                menu(point())
                keyboard.key("j", shift=True); keyboard.key("r")
                keyboard.click((point()[0] + 50, point()[1] + 16))
                assert keyboard.clipboard() == URL, "menu ignores reader navigation and rotation keys"
                expect(0)
                for digit in "99":
                    keyboard.key(digit)
                menu(point()); keyboard.key(0xFF1B)
                keyboard.key("j", shift=True)
                expect(1)
                keyboard.key("k", shift=True)
                expect(0)
                keyboard.key("y")
                menu(point())
                keyboard.key(0xFF54); keyboard.key(0xFF0D)
                assert keyboard.clipboard() == URL, "arrow and Enter activate the menu item"
                keyboard.key("y")
                menu(point())
                keyboard.key(0xFF09); keyboard.key(0xFF09, shift=True)
                keyboard.key(0xFF51); keyboard.key(0xFF53)
                keyboard.key(0xFF0D)
                assert keyboard.clipboard() == URL, "Tab and directional focus stay inside the menu"
                keyboard.key("v")
                menu(point()); keyboard.key("j", shift=True)
                expect(1)
                keyboard.key("k", shift=True)
                expect(0)
                keyboard.key("r")
                expect(0, 1)
                keyboard.key("y")
                copy_menu(point(rotation=1))
                keyboard.key("r", shift=True)
                expect(0)
                keyboard.key("f", ctrl=True)
                for character in "jkgquery":
                    keyboard.key(character, delay=0)
                menu(point(search=True)); keyboard.key(0xFF1B)
                keyboard.key("+", shift=True)
                wait_for(lambda: isinstance(book().get("fit"), dict), "zoom after closing only the menu")
                assert abs(book()["fit"]["Custom"] - (820 - 133 - 42) / 800 * 1.2) < 0.01, "Escape must leave the underlying search bar open"
                keyboard.key(0xFF1B); keyboard.key("p")
                menu(point())
                keyboard.key("f", ctrl=True)
                keyboard.key("a", ctrl=True)
                for character in "menuquery":
                    keyboard.key(character, delay=0)
                keyboard.key("a", ctrl=True); keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == "menuquery", "Find closes the menu and focuses its input"
                keyboard.key(0xFF1B)
                keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_uint, ctypes.c_uint]
                keyboard.x11.XResizeWindow(keyboard.display, window, 860, 540)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.4)
                copy_menu(point(860, 540))
                menu(point(860, 540))
                keyboard.key("w", ctrl=True)
                keyboard.key("p", ctrl=True)
                for character in "pagewing-smoke":
                    keyboard.key(character, delay=0)
                keyboard.key(0xFF0D)
                time.sleep(0.5)
                keyboard.key("j", shift=True)
                expect(1)
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                print("PASS: web-link menu copy, reader-key isolation, count reset, arrows/Enter and Tab focus isolation, selection isolation, rotated and compact links, Escape preserves search, Find focus, close/reopen and graceful quit")
            except Exception:
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
