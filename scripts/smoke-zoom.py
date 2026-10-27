#!/usr/bin/env python3
"""Exercise typed zoom, presets, cancellation and input isolation in the native reader."""
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
ENTER, ESCAPE, TAB = 0xFF0D, 0xFF1B, 0xFF09


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-zoom-") as directory:
        root = Path(directory)
        document = root / "pagewing-smoke.pdf"
        document.write_bytes((SCRIPTS.parent / "assets/pagewing-guide.pdf").read_bytes())
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

            def expect(scale, page=0, continuous=False, rotation=0):
                def matches():
                    saved = book()
                    return (saved.get("fit") == {"Custom": scale} and saved.get("page") == page
                            and saved.get("continuous") == continuous and saved.get("rotation") == rotation
                            and not saved.get("bookmarks") and not saved.get("marks"))
                wait_for(matches, f"zoom {scale}, page {page + 1}, continuous {continuous}, rotation {rotation}")

            def text(value, delay=0.025):
                for character in value:
                    keyboard.key(character.lower(), shift=character.isupper() or character == "%", delay=delay)

            def open_zoom(click=False):
                if click:
                    keyboard.click((1117, 802))
                else:
                    keyboard.key("z", ctrl=True, alt=True)

            def replace(value):
                keyboard.key("a", ctrl=True)
                text(value)

            def copied():
                keyboard.key("a", ctrl=True)
                keyboard.key("c", ctrl=True)
                return keyboard.clipboard()

            def burst(value, click=False, shortcut=False, escape=False):
                application.send_signal(signal.SIGSTOP)
                try:
                    if click:
                        open_zoom(click=True)
                    if shortcut:
                        keyboard.key("z", ctrl=True, alt=True, delay=0)
                    if escape:
                        keyboard.key(ESCAPE, delay=0)
                    text(value, delay=0)
                    if not escape:
                        keyboard.key(ENTER, delay=0)
                finally:
                    application.send_signal(signal.SIGCONT)
                time.sleep(0.3)

            try:
                window = wait_for(keyboard.focus, "zoom reader")
                wait_for(lambda: book().get("fit") == "Page", "initial fit-page view")
                open_zoom(click=True)
                keyboard.key("c", ctrl=True)
                assert float(keyboard.clipboard()) > 0, "opening selects the current percentage"
                text("125%")
                keyboard.key(ENTER)
                expect(1.25)
                assert abs(book()["offset"][1] - 0.5) < 0.001, "typed zoom preserves the view center"

                open_zoom()
                text("jkg0%")
                keyboard.key(ENTER)
                assert copied() == "jkg0%", "invalid input stays editable and cannot trigger reader commands"
                expect(1.25)
                # Tab from the field activates the first preset.
                keyboard.key(TAB); keyboard.key(ENTER)
                expect(0.5)

                open_zoom()
                text("200")
                # Field, four presets, Apply, Cancel.
                for _ in range(6):
                    keyboard.key(TAB)
                keyboard.key(ENTER)
                expect(0.5)
                keyboard.key("j", shift=True)
                expect(0.5, page=1)
                keyboard.key("k", shift=True)
                expect(0.5)

                burst("175%", click=True)
                expect(1.75)
                burst("225%", shortcut=True)
                expect(2.25)
                open_zoom()
                text("250")
                open_zoom()
                assert copied() == "250", "repeating the shortcut preserves the zoom draft"
                burst("J", escape=True)
                expect(2.25, page=1)
                keyboard.key("k", shift=True)
                keyboard.key("c", shift=True)
                open_zoom()
                text("150")
                keyboard.key(ENTER)
                expect(1.5, continuous=True)
                keyboard.key("c", shift=True)
                keyboard.key("r")
                open_zoom()
                text("125")
                keyboard.key(ENTER)
                expect(1.25, rotation=1)

                open_zoom()
                keyboard.key("f", ctrl=True)
                text("zoom")
                assert copied() == "zoom", "Find replaces the zoom input cleanly"
                open_zoom()
                text("175"); keyboard.key(ENTER)
                expect(1.75, rotation=1)
                keyboard.key(ESCAPE)
                expect(1.75, rotation=1)
                open_zoom()
                text("0")
                keyboard.key(ENTER)
                assert copied() == "0", "zero is rejected without losing the input"
                keyboard.key(ESCAPE)
                expect(1.75, rotation=1)
                open_zoom()
                keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_uint, ctypes.c_uint]
                keyboard.x11.XResizeWindow(keyboard.display, window, 240, 240)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)
                text("0"); keyboard.key(ENTER)
                assert copied() == "0", "validation remains editable at the minimum window size"
                replace("150"); keyboard.key(ENTER)
                expect(1.5, rotation=1)
                for tabs, scale in enumerate((0.5, 1.0, 1.5, 2.0), start=1):
                    open_zoom()
                    text("0"); keyboard.key(ENTER)
                    for _ in range(tabs):
                        keyboard.key(TAB)
                    keyboard.key(ENTER)
                    expect(scale, rotation=1)
                open_zoom()
                text("0"); keyboard.key(ENTER)
                for width, height in ((1180, 820), (240, 240), (1180, 240), (240, 820)):
                    keyboard.x11.XResizeWindow(keyboard.display, window, width, height)
                    keyboard.x11.XFlush(keyboard.display)
                    time.sleep(0.2)
                    assert copied() == "0", "resizing preserves the invalid zoom draft and focus"
                    expect(2.0, rotation=1)
                # Apply follows the four presets; the invalid value must keep
                # the dialog open and return focus to its input.
                for _ in range(5):
                    keyboard.key(TAB)
                keyboard.key(ENTER)
                assert copied() == "0", "invalid Apply returns focus to the input"
                for _ in range(6):
                    keyboard.key(TAB)
                keyboard.key(ENTER)
                expect(2.0, rotation=1)
                keyboard.key("j", shift=True)
                expect(2.0, page=1, rotation=1)
                assert application.poll() is None
                print("PASS: selected percentage, typed zoom and center, invalid input isolation, presets, Tab/Enter cancellation, same-frame click/shortcut/text/apply and Escape/navigation, repeated shortcut draft, continuous/rotated zoom, bidirectional Find handoff, minimum-size presets, resize-preserved invalid draft, Apply validation and Cancel focus release")
            except Exception:
                log.flush(); log.seek(0)
                print(log.read(), file=sys.stderr)
                if config_path.exists():
                    print(config_path.read_text(), file=sys.stderr)
                raise
            finally:
                application.terminate(); application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
