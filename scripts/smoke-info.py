#!/usr/bin/env python3
"""Check document information without a display or persistent reader state."""
import os
from pathlib import Path
import runpy
import subprocess
import sys
import tempfile

SCRIPTS = Path(__file__).resolve().parent
metadata_pdf = runpy.run_path(str(SCRIPTS / "smoke-properties.py"))["metadata_pdf"]
encrypted_pdf = runpy.run_path(str(SCRIPTS / "make-encrypted.py"))["make_pdf"]


def run(binary):
    with tempfile.TemporaryDirectory(prefix="folio-info-") as directory:
        root = Path(directory)
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"))
        mac_state = Path.home() / "Library/Application Support/dev.folio.Folio/library.json"
        mac_before = mac_state.read_bytes() if sys.platform == "darwin" and mac_state.exists() else None
        for name in ("DISPLAY", "WAYLAND_DISPLAY", "FOLIO_FONT"):
            environment.pop(name, None)
        source = (SCRIPTS.parent / "assets/folio-guide.pdf").read_bytes()
        document = root / "-中文 notes.pdf"
        document.write_bytes(metadata_pdf(source, "中文 😀 reading"))
        before = document.read_bytes()

        def invoke(arguments, error=None):
            result = subprocess.run([str(binary), *map(str, arguments)], cwd=root,
                                    env=environment, capture_output=True, text=True, timeout=30)
            if error is None:
                assert result.returncode == 0 and not result.stderr, (arguments, result)
            else:
                assert result.returncode != 0 and error in result.stderr, (arguments, result)
                assert not result.stdout, (arguments, result.stdout)
            return result.stdout

        text = invoke(["--info", document.name])
        assert text.splitlines() == [
            f"File: {document.name}", "Format: PDF 1.4", f"Size: {len(before)} bytes", "Pages: 6",
            "Title: 中文 😀 reading", "Author: Author—Name", "Subject: Reading", "Creator: Folio smoke",
        ], text
        assert document.read_bytes() == before
        clean = root / "clean.pdf"
        clean.write_bytes(source)
        assert invoke(["--info", clean]).splitlines() == [
            f"File: {clean}", "Format: PDF 1.4", f"Size: {len(source)} bytes", "Pages: 6",
        ]
        long_title = "中😀" * 3000
        document.write_bytes(metadata_pdf(source, long_title))
        text = invoke(["--info", document])
        title = next(line.removeprefix("Title: ") for line in text.splitlines() if line.startswith("Title: "))
        assert title == long_title[:2048] + "…", title
        if os.name == "posix":
            raw_path = os.fsencode(root) + b"/raw-\xff.pdf"
            with open(raw_path, "wb") as output:
                output.write(source)
            assert "Pages: 6\n" in invoke(["--info", os.fsdecode(raw_path)])

        missing = root / "missing.pdf"
        invoke(["--info"], "Missing PDF path")
        invoke(["--info", missing, "extra"], "Too many arguments")
        invoke(["--info", missing], "Cannot read")
        invalid = root / "invalid.pdf"
        invalid.write_bytes(b"not a PDF")
        invoke(["--info", invalid], "Cannot open PDF")
        encrypted = root / "encrypted.pdf"
        encrypted.write_bytes(encrypted_pdf())
        invoke(["--info", encrypted], "Decryption")
        reader, writer = os.pipe()
        os.close(reader)
        try:
            result = subprocess.run([str(binary), "--info", str(clean)], env=environment,
                                    stdout=writer, stderr=subprocess.PIPE, text=True, timeout=30)
        finally:
            os.close(writer)
        assert result.returncode == 0 and not result.stderr, result
        if sys.platform.startswith("linux"):
            with open("/dev/full", "wb") as full:
                result = subprocess.run([str(binary), "--info", str(clean)], env=environment,
                                        stdout=full, stderr=subprocess.PIPE, text=True, timeout=30)
            assert result.returncode != 0 and "Cannot write standard output" in result.stderr, result
        assert not (root / "config").exists() and not (root / "data").exists()
        if sys.platform == "darwin":
            mac_after = mac_state.read_bytes() if mac_state.exists() else None
            assert mac_after == mac_before, "headless inspection must not persist reader state"
        print("PASS: headless document information, Unicode and bounded metadata, original special paths, missing/invalid/encrypted documents, argument validation, closed pipes, stdout errors and no reader state")


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/folio").resolve())
