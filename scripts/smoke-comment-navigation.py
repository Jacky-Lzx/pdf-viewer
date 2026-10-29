#!/usr/bin/env python3
"""Check virtual comment selection, resizing and manual scrolling on X11."""
import os
from pathlib import Path
import runpy
import signal
import subprocess
import sys
import tempfile

SCRIPTS = Path(__file__).resolve().parent
desktop = runpy.run_path(str(SCRIPTS / "smoke-desktop.py"))
Keyboard, wait_for = desktop["Keyboard"], desktop["wait_for"]
assert_stays = desktop["assert_stays"]
comments_pdf = runpy.run_path(str(SCRIPTS / "smoke-comments.py"))["comments_pdf"]


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-comment-navigation-") as directory:
        root = Path(directory)
        document = root / "pagewing-smoke.pdf"
        original = comments_pdf(rows=80)
        document.write_bytes(original)
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment,
                                           stdout=log, stderr=log)

            def resize(size):
                keyboard.resize_reader(window, *size)

            def selected_rows(size):
                width, height = size
                rows = []
                # The outer panel can scroll filters above the visible list.
                gaps = (20, 40, 60) if width < 400 else (40, 80, 120)
                with keyboard.frame((width - max(gaps), 60, max(gaps), height - 105)) as frame:
                    for y in range(60, height - 45, 2):
                        colors = [frame.pixel((width - gap, y)) for gap in gaps]
                        if all(g > r + 8 and g > b + 6 for r, g, b in colors):
                            rows.append(y)
                return rows

            def selected_card(size):
                # A broad fill spanning most of a card excludes input borders
                # and isolated green text, and catches cards clipped on resize.
                return len(selected_rows(size)) >= 23

            def copied():
                keyboard.key("a", ctrl=True)
                keyboard.key("c", ctrl=True)
                return keyboard.clipboard()

            def expect_body(expected, description):
                actual = None

                def matches():
                    nonlocal actual
                    actual = copied()
                    return actual == expected

                try:
                    wait_for(matches, description)
                except AssertionError as error:
                    raise AssertionError(f"{description}: expected {expected!r}, got {actual!r}") from error

            def filter_comments(compact=False):
                keyboard.key("m", ctrl=True, shift=True)
                if compact:
                    # A restored narrow sidebar uses a shorter title row even
                    # in a wide window. Exercise that layout explicitly.
                    wait_for(lambda: keyboard.input_position((1100, 90, 70, 55)),
                             "comment sidebar appears before narrowing")
                    with keyboard.frame((800, 400, 380, 1)) as frame:
                        background = frame.pixel((1170, 400))
                        left = next(x for x in range(800, 1080)
                                    if frame.pixel((x, 400)) == background)
                    keyboard.drag((left, 400), (960, 400))
                focus_filter()
                for char in "Row":
                    keyboard.key(char.lower(), shift=char.isupper())
                assert copied() == "Row", "the comment filter must own the actual query before navigation"

            def focus_filter():
                # Include the field's empty right end and adjacent panel margin.
                keyboard.focus_input((1100, 90, 70, 55), "comment filter input bounds")

            def burst(keys):
                application.send_signal(signal.SIGSTOP)
                try:
                    for key, control in keys:
                        keyboard.key(key, ctrl=control, delay=0)
                finally:
                    application.send_signal(signal.SIGCONT)

            try:
                window = wait_for(keyboard.focus, "comment navigation window")
                resize((1180, 820))
                filter_comments()
                for edge, arrow in [(0xFF57, 0xFF52), (0xFF50, 0xFF54)]:
                    keyboard.key(edge)
                    keyboard.key(arrow, shift=True)
                    keyboard.key("c", ctrl=True)
                    assert keyboard.clipboard() == "Row", "Shift+arrows select comment filter text"
                keyboard.key(0xFF57)
                burst([(0xFF50, True), (0xFF56, False), (0xFF0D, False)])
                expect_body("Row 7", "PageDown selects seven visible cards before Enter")
                focus_filter()
                burst([(0xFF57, True), (0xFF56, False), (0xFF52, False), (0xFF0D, False)])
                expect_body("Row 78", "boundary actions clamp in order with an editor open")
                focus_filter()
                burst([(0xFF57, True), (0xFF55, False), (0xFF0D, False)])
                expect_body("Row 78", "PageUp moves by one fully visible card in the editor layout")
                assert document.read_bytes() == original
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                # Reopen with no editor to exercise the original full-list path.
                application = subprocess.Popen([str(binary), str(document)], env=environment,
                                               stdout=log, stderr=log)
                window = wait_for(keyboard.focus, "reopened comment navigation window")
                resize((1180, 820))
                filter_comments()
                for _ in range(79):
                    keyboard.key("j", ctrl=True, delay=0.015)
                wait_for(lambda: selected_card((1180, 820)), "last comment selected")
                size = (500, 400)
                resize(size)
                wait_for(lambda: selected_card(size), "selected card remains visible after shrinking")
                assert copied() == "Row", "resizing preserves the filter and its input focus"
                rows = selected_rows(size)
                keyboard.wheel((460, rows[len(rows) // 2]), steps=40)
                wait_for(lambda: not selected_card(size), "manual scrolling can hide the selected card")
                assert_stays(lambda: not selected_card(size), "redraw preserves manual scrolling")
                keyboard.key("k", ctrl=True)
                wait_for(lambda: selected_card(size), "keyboard navigation restores selection visibility")
                keyboard.key(0xFF0D)
                wait_for(lambda: copied() == "Row 78", "Enter opens the retained keyboard selection")
                size = (640, 480)
                resize(size)
                wait_for(lambda: selected_card(size), "editor card remains visible after resizing")
                assert copied() == "Row 78", "resizing preserves editor text and focus"
                for size in [(240, 400), (240, 240)]:
                    resize(size)
                    keyboard.key(0xFF50, ctrl=True)
                    keyboard.key(0xFF53, shift=True)
                    keyboard.key("c", ctrl=True)
                    assert keyboard.clipboard() == "R", f"minimum window {size} keeps editor selection active"
                    assert copied() == "Row 78", f"minimum window {size} preserves editor text and focus"
                resize((640, 480))
                wait_for(lambda: selected_card((640, 480)), "selected card returns after minimum window sizes")
                assert document.read_bytes() == original, "navigation does not write the PDF"
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                # Start without an editor so Tab must generate cards beyond
                # the original virtual range, rather than using filter motions.
                application = subprocess.Popen([str(binary), str(document)], env=environment,
                                               stdout=log, stderr=log)
                window = wait_for(keyboard.focus, "reopened comment Tab navigation window")
                resize((1180, 820))
                filter_comments(compact=True)
                for _ in range(10):
                    keyboard.key(0xFF09)
                    if selected_card((1180, 820)):
                        break
                else:
                    raise AssertionError("Tab must reach the first comment card")
                for _ in range(24):
                    keyboard.key(0xFF09)
                wait_for(lambda: selected_card((1180, 820)), "Tab reveals a later virtual card")
                resize((240, 240))
                wait_for(lambda: selected_card((240, 240)), "the focused card stays visible in a minimum-size window")
                keyboard.key(" ")
                expect_body("Row 24", "Space opens the retained virtual card after resizing")
                assert document.read_bytes() == original, "focus traversal does not write the PDF"
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                print("PASS: 80 virtual comments, Shift+arrow text selection, viewport-sized PageUp/PageDown, Ctrl+Home/End and ordered boundary actions, keyboard selection, filter/editor focus, "
                      "full card after resize, minimum window editor focus, manual scroll retention, resumed keyboard navigation and Tab through virtual cards with retained minimum-window activation")
            except Exception:
                log.flush()
                log.seek(0)
                print(log.read(), file=sys.stderr)
                raise
            finally:
                if application.poll() is None:
                    application.terminate()
                    application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
