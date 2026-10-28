#!/usr/bin/env python3
"""Check keyboard selection and virtualized chapter scrolling in a real desktop."""
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
ENTER, ESCAPE = 0xFF0D, 0xFF1B


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-outline-keyboard-") as directory:
        root = Path(directory)
        fixture = runpy.run_path(str(SCRIPTS / "make-sample.py"))
        fixture["PAGES"][:] = [(f"Chapter {page + 1:03}", "CHAPTER", ["Keyboard chapter navigation."]) for page in range(80)]
        document = root / "pagewing-smoke.pdf"
        document.write_bytes(fixture["make_pdf"]())
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/pagewing/library.json"
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)

            def expect(page, description):
                def saved():
                    try:
                        book = json.loads(config_path.read_text())["recent"][0]
                        return book["page"] == page and book["rotation"] == 0
                    except (OSError, ValueError, KeyError, IndexError):
                        return False
                wait_for(saved, description)

            def burst(keys):
                application.send_signal(signal.SIGSTOP)
                try:
                    for symbol, modifiers in keys:
                        keyboard.key(symbol, delay=0, **modifiers)
                finally:
                    application.send_signal(signal.SIGCONT)
                time.sleep(0.4)

            def selected_row_visible(bounds=(64, 486), xs=(50, 80, 110)):
                # The outer sidebar can retain a small scroll offset while the
                # entire chapter row remains visible. Include that position,
                # but require a continuous band so an input border cannot pass.
                # Chapter rows are indented; selected page rows also fill x=20.
                def green(color):
                    red, green, blue = color
                    return green > red + 8 and green > blue + 6

                filled = 0
                for y in range(*bounds, 2):
                    colors = [keyboard.pixel((x, y)) for x in xs]
                    if all(map(green, colors)) and not green(keyboard.pixel((20, y))):
                        filled += 1
                        if filled >= 6:
                            return True
                    else:
                        filled = 0
                return False

            try:
                window = wait_for(keyboard.focus, "long-outline reader window")
                expect(0, "initial document")
                burst([("c", {})] + [(letter, {}) for letter in "chapter"])
                for edge, arrow in [(0xFF57, 0xFF52), (0xFF50, 0xFF54)]:
                    keyboard.key(edge)
                    keyboard.key(arrow, shift=True)
                    keyboard.key("c", ctrl=True)
                    assert keyboard.clipboard() == "chapter", "Shift+arrows select chapter filter text"
                keyboard.key(ESCAPE)
                burst([("c", {}), (0xFF50, {"ctrl": True}), (0xFF56, {}), (ENTER, {})])
                expect(7, "PageDown selects seven visible chapter rows before confirming")
                burst([("c", {}), (0xFF57, {"ctrl": True}), (0xFF56, {}), (0xFF52, {}), (ENTER, {})])
                expect(78, "Ctrl+End then PageDown then Up clamps each event in order")
                burst([("c", {}), (0xFF50, {"ctrl": True}), (0xFF55, {}), (0xFF54, {}), (ENTER, {})])
                expect(1, "Ctrl+Home then PageUp then Down clamps each event in order")
                burst([("c", {}), (0xFF50, {"ctrl": True}), (ENTER, {})])
                expect(0, "Ctrl+Home restores the first chapter")
                burst([("c", {}), ("k", {"ctrl": True}), ("j", {"ctrl": True})])
                keyboard.key(ENTER)
                expect(1, "same-frame up at the first chapter then down keeps event order")
                burst([("c", {}), ("a", {"ctrl": True})]
                      + [(letter, {}) for letter in "080 chapter"] + [(ENTER, {})])
                expect(79, "reordered chapter keywords match the same title before Enter")
                burst([("c", {}), ("a", {"ctrl": True}), (0xFF08, {})]
                      + [("j", {"ctrl": True})] * 79)
                wait_for(selected_row_visible, "last of 80 selected chapters is drawn")
                keyboard.key(ENTER)
                expect(79, "Enter follows the final selected chapter")
                burst([("c", {}), (0xFF54, {}), (0xFF52, {})])
                keyboard.key(ENTER)
                expect(78, "same-frame down at the last chapter then up keeps event order")
                keyboard.key("k", shift=True)
                expect(77, "chapter confirmation releases reading keys")
                keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_uint, ctypes.c_uint]
                keyboard.x11.XResizeWindow(keyboard.display, window, 860, 540)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)
                burst([("c", {})] + [(0xFF52, {})] * 79)
                wait_for(selected_row_visible, "first chapter selection is drawn in the compact window")
                keyboard.key(ENTER)
                expect(0, "first chapter remains reachable after virtual scrolling")
                burst([("c", {}), ("a", {"ctrl": True})] + [(letter, {}) for letter in "080 chapter"])
                wait_for(selected_row_visible, "filtering finds the final title without old row offsets")
                keyboard.key(ENTER)
                expect(79, "filtered last chapter follows its real destination")
                keyboard.key(ESCAPE)
                keyboard.wheel((200, 480), down=True, steps=25)
                burst([("c", {}), ("a", {"ctrl": True})] + [(letter, {}) for letter in "001 chapter"])
                wait_for(selected_row_visible, "chapter focus reveals the list after the outer sidebar was scrolled")
                keyboard.key(ENTER)
                expect(0, "chapter selection works after scrolling the outer sidebar")
                keyboard.x11.XResizeWindow(keyboard.display, window, 860, 240)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)
                burst([("c", {}), ("a", {"ctrl": True}), (0xFF08, {})]
                      + [(0xFF54, {})] * 39)
                wait_for(lambda: selected_row_visible((120, 190)),
                         "middle chapter is visible in a short window")
                for width, height in ((240, 240), (1180, 820), (240, 240)):
                    keyboard.x11.XResizeWindow(keyboard.display, window, width, height)
                    keyboard.x11.XFlush(keyboard.display)
                    time.sleep(0.3)
                    narrow = width == 240
                    wait_for(lambda: selected_row_visible(
                        (120, 190) if narrow else (228, 486),
                        (45, 65, 85) if narrow else (50, 80, 110)),
                        "resizing preserves the visible chapter selection")
                keyboard.key(ENTER)
                expect(39, "minimum-size chapter confirmation follows the retained selection")
                keyboard.key("y")
                assert "Chapter 040" in keyboard.clipboard()
                burst([("c", {}), ("a", {"ctrl": True})]
                      + [(letter, {}) for letter in "080 chapter"])
                wait_for(lambda: selected_row_visible((120, 190), (45, 65, 85)),
                         "filtered chapter remains visible at minimum size")
                keyboard.key(ENTER)
                expect(79, "minimum-size filter follows the final chapter")
                burst([("c", {}), ("a", {"ctrl": True}), (0xFF08, {})]
                      + [(0xFF52, {})] * 79)
                wait_for(lambda: selected_row_visible((120, 190), (45, 65, 85)),
                         "first chapter remains visible at minimum size")
                keyboard.key(ENTER)
                expect(0, "minimum-size navigation reaches the first chapter")
                burst([("c", {}), (0xFF50, {"ctrl": True}), (0xFF56, {}), (ENTER, {})])
                expect(2, "minimum-size PageDown moves by two visible chapter rows")
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                print("PASS: 80 chapter rows, Shift+arrow text selection, viewport-sized PageUp/PageDown and Ctrl+Home/End, ordered boundary actions, reordered multiword filters, first/last keyboard navigation, actual selection visibility, compact/short/minimum layouts, selection retained across resizing, exact destination text, filtering after scrolling and outer sidebar focus")
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
