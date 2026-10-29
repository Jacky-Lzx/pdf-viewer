#!/usr/bin/env python3
"""Exercise long Unicode comment filtering, replacement and deferred Enter."""
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
comments_pdf = runpy.run_path(str(SCRIPTS / "smoke-comments.py"))["comments_pdf"]


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-comment-filter-load-") as directory:
        root = Path(directory)
        document = root / "pagewing-smoke.pdf"
        body = "Straße 中文 Σ ﬁ " * 2048 + " terminalneedle"
        original = comments_pdf(rows=2000, row_contents=body)
        document.write_bytes(original)
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment,
                                           stdout=log, stderr=log)

            def text(value, delay=0.01):
                for character in value:
                    keyboard.key(character.lower(), shift=character.isupper(), delay=delay)

            def copied():
                keyboard.key("a", ctrl=True)
                keyboard.key("c", ctrl=True)
                return keyboard.clipboard()

            def query(value, confirm=True, pending_text=""):
                bounds = (1100, 90, 70, 55)
                if not pending_text:
                    position = keyboard.focus_input(bounds, "visible comment filter bounds")
                else:
                    # The click stays in the stopped native batch after draft text.
                    position = wait_for(lambda: keyboard.input_position(bounds), "visible comment filter bounds")
                application.send_signal(signal.SIGSTOP)
                try:
                    if pending_text:
                        text(pending_text, delay=0)
                        keyboard.click(position)
                    keyboard.key("a", ctrl=True, delay=0)
                    keyboard.key(0xFF08, delay=0)
                    text(value, delay=0)
                    if confirm:
                        keyboard.key(0xFF0D, delay=0)
                finally:
                    application.send_signal(signal.SIGCONT)

            def expect_body(value, description):
                actual = None

                def matches():
                    nonlocal actual
                    actual = copied()
                    return actual == value

                try:
                    wait_for(matches, description)
                except AssertionError as error:
                    raise AssertionError(f"{description}: expected {len(value)} characters, got {None if actual is None else (len(actual), actual[:120])!r}") from error

            def result_ready():
                rows = 0
                with keyboard.frame((1060, 220, 81, 220)) as frame:
                    for y in range(220, 440, 2):
                        colors = [frame.pixel((x, y)) for x in (1060, 1100, 1140)]
                        rows += all(g > r + 8 and g > b + 6 for r, g, b in colors)
                return rows >= 23

            def editor_position():
                with keyboard.frame((1150, 320, 21, 370)) as frame:
                    background = frame.pixel((1170, 400))
                    bands = frame.bands([1150], lambda color: color != background, min_height=65)
                    if bands:
                        top, end = bands[0]
                        return 1150, (top + end - 1) // 2
                return None

            try:
                wait_for(keyboard.focus, "long-comment reader window")
                keyboard.key("m", ctrl=True, shift=True)
                query("terminalneedle")
                expect_body(body, "Enter waits for complete Unicode filtering and opens the first direct match")
                keyboard.key(0xFF57, ctrl=True)
                text(" draft")
                expect_body(body + " draft", "long-body editing reaches the final character")
                keyboard.key("z", ctrl=True)
                expect_body(body, "undo restores the complete long body without truncation")
                keyboard.key(0xFF50, ctrl=True)
                keyboard.key(0xFF53, shift=True)
                keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == "S", "Ctrl+Home reaches the first character"
                keyboard.key(0xFF57, ctrl=True)
                keyboard.key(0xFF51, shift=True)
                keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == "e", "Ctrl+End reaches the last character"
                # No outer-panel scrolling: the filter remains visible while
                # the editor owns a caret at the end of its long body.
                query("absentmarker", confirm=False)
                assert copied() == "absentmarker", "the replacement query reaches its field before opening help"
                started = time.monotonic()
                keyboard.click((1140, 28))  # Toolbar help works while the filter owns input.
                text("zoom")
                actual = copied()
                assert actual == "zoom", f"shortcut help takes input while the long filter runs: {actual[:120]!r}"
                print(f"help input while filtering: {time.monotonic() - started:.3f} s", flush=True)
                keyboard.key(0xFF1B)
                query("Alice")
                expect_body("Root question", "a replacement query cancels stale work and opens its own result")
                keyboard.key(0xFF57, ctrl=True)
                query("terminalneedle", pending_text=" draft")
                assert copied() == "terminalneedle", "the new query owns input while the draft is protected"
                wait_for(result_ready, "complete filter publishes its selected card before deferred activation")
                keyboard.click(wait_for(editor_position, "retained draft editor bounds"))
                expect_body("Root question draft", "deferred Enter preserves the unsaved editor draft")
                # Restore the saved text so the ordinary quit remains explicit.
                text("Root question")
                assert copied() == "Root question"
                assert document.read_bytes() == original, "filtering and deferred activation never write the PDF"
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                print("PASS: 2000 long Unicode comments, same-frame query/Enter, full body, long-body editing/undo and first/last-character navigation with reachable filters, help during filtering, query cancellation, replacement selection and deferred confirmation protecting an unsaved draft")
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
