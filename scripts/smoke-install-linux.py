#!/usr/bin/env python3
"""Validate user installation and GIO launch, including Desktop Entry escaping."""
import json
import os
from pathlib import Path
import runpy
import shutil
import signal
import subprocess
import sys
import tempfile

SCRIPTS = Path(__file__).resolve().parent
INSTALLER = SCRIPTS / "install-linux.sh"
desktop_tools = runpy.run_path(str(SCRIPTS / "smoke-desktop.py"))
Keyboard, wait_for = desktop_tools["Keyboard"], desktop_tools["wait_for"]


def install(arguments, environment, cwd=None, success=True):
    result = subprocess.run(["bash", str(INSTALLER), *arguments], env=environment, cwd=cwd,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    assert (result.returncode == 0) == success, (result.returncode, result.stdout, result.stderr)
    return result


def launch(entry, document, environment, log):
    result = subprocess.run(["gio", "launch", str(entry), str(document)], env=environment,
                            stdout=log, stderr=log, timeout=8)
    if result.returncode:
        log.flush(); log.seek(0)
        raise AssertionError(f"GIO launch failed: {log.read()}\n{entry.read_text()}")


def fake_application(path, generation):
    path.write_text("#!/usr/bin/env python3\nimport json,os,sys\n"
                    f"json.dump({{'generation': {generation}, 'args': sys.argv[1:]}}, "
                    "open(os.environ['FOLIO_INSTALL_CAPTURE'], 'w'))\n")
    path.chmod(0o755)


def captured(path):
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError):
        return None


def process_for(binary):
    for candidate in Path("/proc").iterdir():
        if candidate.name.isdecimal():
            try:
                if os.readlink(candidate / "exe") in [str(binary), f"{binary} (deleted)"]:
                    return int(candidate.name)
            except OSError:
                pass
    return None


def run(binary):
    assert shutil.which("gio"), "Install libglib2.0-bin for the GIO launcher"
    validator = shutil.which("desktop-file-validate")
    assert validator, "Install desktop-file-utils for Desktop Entry validation"
    with tempfile.TemporaryDirectory(prefix="folio-install-") as directory:
        root = Path(directory)
        source = root / "fixture-program"
        environment = dict(os.environ, FOLIO_BINARY=str(source),
                           XDG_CONFIG_HOME=str(root / "config"))
        sentinel = root / "config/folio/keep-reading-progress"
        sentinel.parent.mkdir(parents=True)
        sentinel.write_text("keep")
        invalid = root / "must-not-create"
        install(["--help"], environment)
        install(["--prefix"], environment, success=False)
        install(["--prefix", "--uninstall"], environment, success=False)
        install(["--unknown"], environment, success=False)
        install(["--prefix", str(invalid)], environment, success=False)
        assert not invalid.exists(), "missing release build must fail before changing the prefix"
        fake_application(source, 1)
        install(["--prefix", str(invalid)], dict(environment, XDG_DATA_HOME="relative"), success=False)
        assert not invalid.exists()

        with open(root / "launch.log", "w+") as log:
            special = '应用 space %f %u $HOME `literal` $(literal) "quote" \\ slash\nline\ttab'
            for case, prefix_name, explicit_data in [
                ("ordinary", "ordinary", True),
                ("escaped", special, True),
                ("prefix-data", "relative prefix", False),
            ]:
                prefix = root / prefix_name
                data = root / f"data {case}" / special if explicit_data else prefix / "share"
                capture = root / f"{case}.json"
                current = dict(environment, FOLIO_INSTALL_CAPTURE=str(capture))
                if explicit_data:
                    current["XDG_DATA_HOME"] = str(data)
                else:
                    current.pop("XDG_DATA_HOME", None)
                document = root / ("-文档 %u " + os.fsdecode(b"native-\xff.pdf"))
                document.write_bytes(b"argument fixture")
                installed = prefix / "bin/folio"
                entry = data / "applications/folio.desktop"
                icon = data / "icons/hicolor/scalable/apps/folio.svg"
                # Relative --prefix is resolved against the caller, not the repository.
                arguments = ["--prefix", prefix_name]
                install(arguments, current, cwd=root)
                assert installed.read_bytes() == source.read_bytes()
                assert installed.stat().st_mode & 0o777 == 0o755
                assert entry.stat().st_mode & 0o777 == 0o644
                assert icon.read_bytes() == (SCRIPTS.parent / "assets/icon.svg").read_bytes()
                subprocess.run([validator, str(entry)], check=True)
                launch(entry, document, current, log)
                result = wait_for(lambda: captured(capture), "installed desktop app launch")
                assert result == {
                    "generation": 1, "args": ["--", str(document)],
                }
                original_inode = installed.stat().st_ino
                fake_application(source, 2)
                install(arguments, current, cwd=root)
                assert installed.stat().st_ino != original_inode, "upgrades must replace the binary atomically"
                capture.unlink()
                launch(entry, document, current, log)
                result = wait_for(lambda: captured(capture), "updated desktop app launch")
                assert result["generation"] == 2
                assert not list(prefix.rglob("*.tmp.*"))
                assert not list(data.rglob("*.tmp.*"))
                install([*arguments, "--uninstall"], current, cwd=root)
                install([*arguments, "--uninstall"], current, cwd=root)
                assert not any(path.exists() for path in [installed, entry, icon])
                assert sentinel.read_text() == "keep"
                fake_application(source, 1)

            # Exercise the installed Rust binary through the same GIO desktop launcher.
            prefix = root / "native 中文 space %f"
            data = root / "native data"
            current = dict(environment, FOLIO_BINARY=str(binary), XDG_DATA_HOME=str(data),
                           LIBGL_ALWAYS_SOFTWARE="1")
            document = root / "文档 path/-folio-smoke.pdf"
            document.parent.mkdir()
            document.write_bytes((SCRIPTS.parent / "assets/folio-guide.pdf").read_bytes())
            arguments = ["--prefix", str(prefix)]
            install(arguments, current)
            installed = prefix / "bin/folio"
            keyboard = Keyboard()
            pid = None
            try:
                launch(data / "applications/folio.desktop", document, current, log)
                pid = wait_for(lambda: process_for(installed), "installed native process")
                wait_for(keyboard.focus, "desktop-opened PDF window")
                keyboard.key("y")
                assert "A little room to read" in keyboard.clipboard()
                keyboard.key("j", shift=True)
                keyboard.key("y")
                assert "NAVIGATION" in keyboard.clipboard(), "installed app retains Vim navigation"
                install(arguments, current)
                assert process_for(installed) == pid, "reinstall must leave the running reader alive"
                keyboard.key("k", shift=True); keyboard.key("y")
                assert "A little room to read" in keyboard.clipboard(), "reader stays usable after upgrade"
                keyboard.key("q", ctrl=True)
                wait_for(lambda: process_for(installed) is None, "installed app quit")
            finally:
                if pid and process_for(installed) == pid:
                    os.kill(pid, signal.SIGTERM)
                keyboard.x11.XCloseDisplay(keyboard.display)
                install([*arguments, "--uninstall"], current)
            assert sentinel.read_text() == "keep"
            assert (root / "config/folio/library.json").exists(), "uninstall must preserve actual reading state"
        print("PASS: Desktop Entry validation, user/custom and relative prefixes, XDG paths, "
              "Unicode/space/quote/backslash/percent/control characters, exact native filename arguments, "
              "atomic upgrades, reinstall while reading, native PDF launch and Vim navigation, "
              "idempotent uninstall and preserved configuration")


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/folio").resolve())
