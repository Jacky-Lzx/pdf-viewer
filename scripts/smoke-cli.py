#!/usr/bin/env python3
"""Check CLI validation, real exports and native opening of a dash-prefixed path."""
import json
import os
from pathlib import Path
import runpy
import struct
import subprocess
import sys
import tempfile

SCRIPTS = Path(__file__).resolve().parent
desktop = runpy.run_path(str(SCRIPTS / "smoke-desktop.py"))
Keyboard, wait_for = desktop["Keyboard"], desktop["wait_for"]


def run(binary):
    with tempfile.TemporaryDirectory(prefix="folio-cli-") as directory:
        root = Path(directory)
        document = root / "-文档 notes.pdf"
        document.write_bytes((SCRIPTS.parent / "assets/folio-guide.pdf").read_bytes())
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/folio/library.json"

        def invoke(args, expected_error=None):
            result = subprocess.run([str(binary), *map(str, args)], cwd=root, env=environment, text=True, capture_output=True, timeout=30)
            if expected_error is None:
                assert result.returncode == 0, (args, result.stderr)
            else:
                assert result.returncode != 0 and expected_error in result.stderr, (args, result.returncode, result.stderr)
            return result.stdout

        assert "Folio " in invoke(["--version"])
        assert "folio -- [document.pdf]" in invoke(["--help"])
        missing = root / "missing.pdf"
        output = root / "page.png"
        for args, error in [
            (["--render", missing], "Missing output PNG path"),
            (["--render", missing, output, "0"], "Page numbers start at 1"),
            (["--render", missing, output, "1", "16385"], "Width must be between"),
            (["--benchmark", missing, "0"], "Iterations must be between"),
            (["--text", missing, "1", "extra"], "Too many"),
            ([document, document], "Open one PDF at a time"),
            ([document.name], "Unknown option"),
            (["--text", missing, "1"], "Cannot read"),
        ]:
            invoke(args, error)
        assert not output.exists(), "invalid commands must not create output files"
        assert "READING" in invoke(["--text", document, "3"])
        invoke(["--render", document, output, "1", "64"])
        data = output.read_bytes()
        assert data[:8] == b"\x89PNG\r\n\x1a\n"
        assert struct.unpack(">II", data[16:24]) == (64, 85)
        assert "6 pages × 1 passes" in invoke(["--benchmark", document, "1"])

        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), "--", document.name], cwd=root, env=environment, stdout=log, stderr=log)

            def saved_page(page):
                def saved():
                    try:
                        book = json.loads(config_path.read_text())["recent"][0]
                        return book["path"] == str(document) and book["page"] == page
                    except (OSError, ValueError, KeyError, IndexError):
                        return False
                return saved

            try:
                wait_for(lambda: keyboard.focus(document.name), "native window for a dash-prefixed Unicode filename")
                wait_for(saved_page(0), "relative filename opens after the option terminator")
                keyboard.key("j", shift=True)
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                assert saved_page(1)(), "Vim navigation and final state save work for the special path"
                print("PASS: argument errors before PDF I/O, version/help, real text/PNG/benchmark output and native -- opening of a relative dash-prefixed Unicode path")
            except Exception:
                log.flush(); log.seek(0)
                print(log.read(), file=sys.stderr)
                raise
            finally:
                if application.poll() is None:
                    application.terminate(); application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/folio").resolve())
