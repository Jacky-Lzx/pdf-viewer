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
            (["--page"], "Missing page number"),
            (["--page", "0", missing], "Page numbers start at 1"),
            (["--page", "2"], "Missing PDF path"),
            (["--page", "2", "--"], "Missing PDF path"),
            (["--page", "2", missing, document], "Open one PDF at a time"),
        ]:
            invoke(args, error)
        assert not output.exists(), "invalid commands must not create output files"
        source_bytes = document.read_bytes()
        aliases = [document, Path(".") / document.name]
        hard = root / "same-inode.png"
        hard.hardlink_to(document)
        link = root / "same-target.png"
        link.symlink_to(document)
        aliases += [hard, link]
        for alias in aliases:
            invoke(["--render", document, alias, "1", "32"],
                   "Cannot export PNG over the source PDF")
            assert document.read_bytes() == source_bytes, "failed export must preserve the source PDF"
        invoke(["--render", link, document, "1", "32"],
               "Cannot export PNG over the source PDF")
        assert document.read_bytes() == source_bytes
        assert "READING" in invoke(["--text", document, "3"])
        invoke(["--render", document, output, "1", "64"])
        data = output.read_bytes()
        assert data[:8] == b"\x89PNG\r\n\x1a\n"
        assert struct.unpack(">II", data[16:24]) == (64, 85)
        assert data.endswith(b"\0\0\0\0IEND\xaeB`\x82")
        if sys.platform.startswith("linux"):
            # A tiny image stays inside BufWriter until the final flush.
            # Dropping the encoder used to swallow ENOSPC and report success.
            invoke(["--render", document, "/dev/full", "1", "1"], "Cannot write /dev/full")
        assert "6 pages × 1 passes" in invoke(["--benchmark", document, "1"])
        pipe_image = root / "pipe.png"
        for args in [
            ["--help"], ["--version"], ["--text", document, "1"],
            ["--benchmark", document, "1"],
            ["--render", document, pipe_image, "1", "32"],
        ]:
            reader, writer = os.pipe()
            os.close(reader)  # No reader exists before the process starts.
            try:
                result = subprocess.run([str(binary), *map(str, args)], cwd=root,
                                        env=environment, stdout=writer, stderr=subprocess.PIPE,
                                        text=True, timeout=30)
            finally:
                os.close(writer)
            assert result.returncode == 0 and not result.stderr, (args, result.returncode, result.stderr)
        assert pipe_image.read_bytes().endswith(b"\0\0\0\0IEND\xaeB`\x82")
        if sys.platform.startswith("linux"):
            with open("/dev/full", "wb") as full:
                result = subprocess.run([str(binary), "--help"], stdout=full,
                                        stderr=subprocess.PIPE, text=True, timeout=30)
            assert result.returncode != 0 and "Cannot write standard output" in result.stderr
            assert "panicked" not in result.stderr

        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            def launch(args):
                return subprocess.Popen([str(binary), *args], cwd=root, env=environment, stdout=log, stderr=log)

            application = launch(["--", document.name])

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
                application = launch(["--page", "4", "--", document.name])
                wait_for(lambda: keyboard.focus(document.name), "native page override window")
                wait_for(saved_page(3), "explicit page overrides the previously saved page")
                keyboard.key("y")
                assert keyboard.clipboard() == invoke(["--text", document, "4"]).removesuffix("\n")
                keyboard.key("b"); keyboard.key("r"); keyboard.key("w")
                for character in "3jma":
                    keyboard.key(character)
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                saved = json.loads(config_path.read_text())["recent"][0]
                assert saved["rotation"] == 1 and saved["fit"] == "Width" and saved["offset"][1] > 0
                assert saved["bookmarks"] == [3] and saved["marks"]["a"]["page"] == 3

                application = launch(["--page", "2", document.name])
                wait_for(lambda: keyboard.focus(document.name), "second page override window")
                wait_for(saved_page(1), "page override resets the old page offset")
                saved = json.loads(config_path.read_text())["recent"][0]
                assert saved["offset"] == [0.0, 0.0] and saved["rotation"] == 1 and saved["fit"] == "Width"
                assert saved["bookmarks"] == [3] and saved["marks"]["a"]["page"] == 3
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0

                application = launch(["--", document.name])
                wait_for(lambda: keyboard.focus(document.name), "ordinary reopen window")
                expected = invoke(["--text", document, "2"]).removesuffix("\n")
                def restored_text():
                    keyboard.key("y")
                    return keyboard.clipboard() == expected
                wait_for(restored_text, "ordinary reopen still restores the last page")
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0

                application = launch(["--page", "999", document.name])
                wait_for(lambda: keyboard.focus(document.name), "clamped start page window")
                wait_for(saved_page(5), "out-of-range GUI start page clamps to the last page")
                keyboard.key("y")
                assert keyboard.clipboard() == invoke(["--text", document, "6"]).removesuffix("\n")
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                print("PASS: CLI validation/exports, closed pipes, real stdout write errors, source PDF preservation through path aliases, special paths, physical start page and clamping, saved-view override, retained zoom/rotation/bookmarks/marks and ordinary session restore")
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
