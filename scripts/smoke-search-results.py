#!/usr/bin/env python3
"""Check search-result visibility and snippet copying in a native X11 window."""
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
ENTER, ESCAPE, TAB, F10 = 0xFF0D, 0xFF1B, 0xFF09, 0xFFC7


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-search-results-") as directory:
        root = Path(directory)
        fixture = runpy.run_path(str(SCRIPTS / "make-sample.py"))
        fixture["PAGES"][:] = [
            (f"Chapter {page + 1:03}", "CHAPTER", [f"Needle {page + 1:03}"])
            for page in range(80)
        ]
        document = root / "pagewing-smoke.pdf"
        document.write_bytes(fixture["make_pdf"]())
        config_path = root / "config/pagewing/library.json"
        config_path.parent.mkdir(parents=True)
        config_path.write_text(json.dumps({
            "sidebar": True,
            "recent": [{"path": str(document), "bookmarks": list(range(60))}],
        }))
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        keyboard = Keyboard()
        keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong,
                                              ctypes.c_uint, ctypes.c_uint]
        keyboard.x11.XSync.argtypes = [ctypes.c_void_p, ctypes.c_int]
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment,
                                           stdout=log, stderr=log)

            def expect(page, description):
                def saved():
                    try:
                        return json.loads(config_path.read_text())["recent"][0]["page"] == page
                    except (OSError, ValueError, KeyError, IndexError):
                        return False
                wait_for(saved, description)

            def resize(width, height):
                keyboard.x11.XResizeWindow(keyboard.display, window, width, height)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)

            def results():
                keyboard.key("f", ctrl=True)
                for _ in range(5):
                    keyboard.key(TAB)
                keyboard.key(ENTER)
                keyboard.key(ESCAPE)

            def burst(keys):
                application.send_signal(signal.SIGSTOP)
                try:
                    _, status = os.waitpid(application.pid, os.WUNTRACED)
                    assert os.WIFSTOPPED(status)
                    for symbol, modifiers in keys:
                        keyboard.key(symbol, delay=0, **modifiers)
                    keyboard.x11.XSync(keyboard.display, False)
                finally:
                    application.send_signal(signal.SIGCONT)
                time.sleep(0.4)

            def selected_result(height):
                # Highlight pixels identify search rows even while bookmarks,
                # chapters and the page list also have selected backgrounds.
                for y in range(112, height - 42, 2):
                    red, green, blue = keyboard.pixel((230, y))
                    if green > red + 8 and green > blue + 6:
                        for x in range(24, 200, 3):
                            if keyboard.pixel((x, y)) == (250, 220, 125):
                                return x, y
                return None

            try:
                window = wait_for(keyboard.focus, "long-outline search window")
                expect(0, "initial page")
                resize(860, 540)
                keyboard.key("f", ctrl=True)
                for character in "Needle":
                    keyboard.key(character.lower(), shift=character.isupper(), delay=0.025)
                keyboard.key(ENTER)
                time.sleep(0.6)
                keyboard.key(ESCAPE)
                keyboard.key("n", shift=True)
                expect(79, "reverse search reaches the last of 80 hits")
                wait_for(lambda: selected_result(540), "last result visible below contents and bookmarks")
                for width, height in ((1180, 820), (860, 240), (860, 540)):
                    resize(width, height)
                    wait_for(lambda: selected_result(height), "selected result retained after resizing")
                keyboard.key("f", ctrl=True)
                for _ in range(6):
                    keyboard.key(TAB)
                keyboard.key(ENTER)
                exported = keyboard.clipboard()
                for required in ("Pagewing search snippets\n", f"File: {document}\n", "Query: Needle\n",
                                 "Available results: 80\n", "[Search complete.]", "Result 1 · Page 1",
                                 "Result 80 · Page 80", "Needle 080", "PDF page label: i\n",
                                 "PDF page label: A-76\n"):
                    assert required in exported, (required, exported)
                expect(79, "copying all results preserves the selected hit and reading page")
                keyboard.key(ESCAPE)
                position = selected_result(540)
                keyboard.click(position, button=3)
                keyboard.key(ENTER)
                wait_for(lambda: "Needle 080" in keyboard.clipboard()
                         and not keyboard.clipboard().startswith("Pagewing search snippets\n"),
                         "single result snippet reaches the system clipboard")
                snippet = keyboard.clipboard()
                assert "Needle 079" not in snippet and "Query:" not in snippet, snippet
                expect(79, "copying a snippet preserves the reading page")
                position = wait_for(lambda: selected_result(540), "copying a snippet preserves the selected match")
                keyboard.click(position, button=3)
                keyboard.key(ESCAPE)
                assert keyboard.clipboard() == snippet, "cancelling the menu changes the clipboard"
                expect(79, "cancelling the snippet menu preserves navigation")

                # Results focuses the selected match without traversing the
                # long outline and bookmarks or changing the reading page.
                keyboard.key("k", shift=True)
                expect(78, "read a different page while retaining the selected match")
                keyboard.key("f", ctrl=True)
                keyboard.key("a", ctrl=True)
                keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == "Needle"
                for _ in range(5):
                    keyboard.key(TAB)
                burst([(ENTER, {}), ("c", {"ctrl": True})])
                assert keyboard.clipboard() == snippet, "Results must focus the selected row before queued copying"
                expect(78, "focusing a result preserves the reading page")
                # Copying belongs to focus, not the selected search match.
                keyboard.key(TAB, shift=True)
                keyboard.key("c", ctrl=True)
                previous_snippet = keyboard.clipboard()
                assert "Needle 079" in previous_snippet and "Needle 080" not in previous_snippet, previous_snippet
                expect(78, "copying a focused neighboring snippet preserves navigation")
                assert selected_result(540), "copying changes the selected search match"
                keyboard.key(TAB)
                burst([(F10, {"shift": True}), (ENTER, {})])
                assert keyboard.clipboard() == snippet, "fast keyboard menu activation must copy the focused result"
                expect(78, "keyboard menu activation preserves navigation")

                burst([(TAB, {"shift": True}), (F10, {"shift": True})])
                for width, height in ((860, 240), (1180, 820), (860, 540)):
                    resize(width, height)
                keyboard.key(ENTER)
                assert keyboard.clipboard() == previous_snippet, "resizing must retain the keyboard snippet menu"
                expect(78, "resized keyboard menu preserves navigation")
                keyboard.key(TAB)
                keyboard.key(F10, shift=True)
                burst([(ESCAPE, {}), ("c", {"ctrl": True})])
                assert keyboard.clipboard() == snippet, "Escape must restore row focus before a queued copy"
                expect(78, "menu cancellation and restored-focus copying preserve navigation")
                keyboard.key(ENTER)
                expect(79, "Enter follows the focused result after Results and copying")
                keyboard.key(ESCAPE)
                position = wait_for(lambda: selected_result(540), "keyboard copying preserves the selected match")
                keyboard.wheel(position, steps=12)
                time.sleep(0.3)
                assert not selected_result(540), "manual list scrolling must remain in place"
                results()
                position = wait_for(lambda: selected_result(540), "Results restores the current row")
                keyboard.key("k", shift=True)
                expect(78, "navigate away without changing the search selection")
                keyboard.click(position)
                expect(79, "the revealed selected row has a working click target")
                keyboard.key("n", shift=True)
                expect(78, "previous search result")
                wait_for(lambda: selected_result(540), "adjacent search result remains visible")
                keyboard.key("c")
                keyboard.key("a", ctrl=True)
                for character in "001 chapter":
                    keyboard.key(character, delay=0.025)
                keyboard.key(ENTER)
                expect(0, "chapter focus still reveals the outline after search navigation")
                results()
                position = wait_for(lambda: selected_result(540), "Results returns from focused contents")
                keyboard.click(position)
                expect(78, "Results preserves the current hit while reading another chapter")
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                print("PASS: 80 search results below long contents and 60 bookmarks, full search-snippet clipboard export, right-click and focused keyboard snippet copying, fast menu activation, resize and restored-focus copying, first/last navigation, manual scrolling, Results restoration, clickable rows and chapter focus")
            except Exception:
                log.flush()
                log.seek(0)
                print(log.read(), file=sys.stderr)
                if config_path.exists():
                    print(config_path.read_text(), file=sys.stderr)
                raise
            finally:
                if application.poll() is None:
                    application.terminate()
                    application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
