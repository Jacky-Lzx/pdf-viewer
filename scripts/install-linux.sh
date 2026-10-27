#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'HELP'
Usage: bash scripts/install-linux.sh [--prefix DIR] [--uninstall]

Install the release binary and a Linux PDF desktop entry for the current user.
The default prefix is ~/.local. Desktop files and icons use XDG_DATA_HOME,
or PREFIX/share when XDG_DATA_HOME is unset. No default PDF app is changed.

Build first with cargo build --release. PAGEWING_BINARY can select another binary.
Use the same prefix and XDG_DATA_HOME to uninstall; reading progress is kept.
HELP
}

prefix=''
uninstall=false
while (($#)); do
    case "$1" in
        --help|-h) usage; exit 0 ;;
        --prefix)
            if (($# < 2)) || [[ -z "$2" || "$2" == --* ]]; then
                echo '--prefix requires a directory.' >&2
                exit 1
            fi
            prefix=$2
            shift 2
            ;;
        --uninstall) uninstall=true; shift ;;
        *) printf 'Unknown argument: %s\n' "$1" >&2; usage >&2; exit 1 ;;
    esac
done
if [[ -z "$prefix" ]]; then
    prefix="${HOME:?HOME is not set}/.local"
fi
if [[ "$(uname -s)" != Linux ]]; then
    echo 'Run this script on Linux; use bundle-macos.sh on macOS.' >&2
    exit 1
fi
if [[ "$prefix" != /* ]]; then
    prefix="$PWD/$prefix"
fi
data="${XDG_DATA_HOME:-"$prefix/share"}"
if [[ "$data" != /* ]]; then
    echo 'XDG_DATA_HOME must be an absolute directory.' >&2
    exit 1
fi
binary="$prefix/bin/pagewing"
desktop="$data/applications/pagewing.desktop"
icon="$data/icons/hicolor/scalable/apps/pagewing.svg"

refresh_desktop_database() {
    if command -v update-desktop-database >/dev/null 2>&1; then
        if ! update-desktop-database "$data/applications"; then
            echo 'Could not refresh the desktop database; the desktop entry was updated.' >&2
        fi
    fi
}

if $uninstall; then
    rm -f -- "$binary" "$desktop" "$icon"
    if [[ -d "$data/applications" ]]; then
        refresh_desktop_database
    fi
    echo 'Removed Pagewing binary, desktop entry and icon. Reading progress was kept.'
    exit 0
fi

repository=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
source_binary="${PAGEWING_BINARY:-"$repository/target/release/pagewing"}"
if [[ ! -f "$source_binary" || ! -x "$source_binary" ]]; then
    printf 'No executable at %s. Run cargo build --release first.\n' "$source_binary" >&2
    exit 1
fi

# Desktop Entry strings are decoded before the Exec command line is parsed.
desktop_string() {
    local value=$1
    value=${value//\\/\\\\}
    value=${value//$'\n'/\\n}
    value=${value//$'\r'/\\r}
    value=${value//$'\t'/\\t}
    printf '%s' "$value"
}

desktop_argument() {
    local value=$1
    value=${value//\\/\\\\}
    value=${value//\"/\\\"}
    value=${value//\$/\\\$}
    value=${value//\`/\\\`}
    value=${value//%/%%}
    printf '"%s"' "$(desktop_string "$value")"
}

temporary=()
trap 'rm -f -- "${temporary[@]}"' EXIT
atomic_copy() {
    local source=$1 destination=$2 mode=$3 stage
    mkdir -p -- "$(dirname -- "$destination")"
    stage=$(mktemp "$destination.tmp.XXXXXX")
    temporary+=("$stage")
    install -m "$mode" -- "$source" "$stage"
    # Replacing the inode also permits upgrading while the old binary is running.
    mv -fT -- "$stage" "$destination"
}

atomic_copy "$source_binary" "$binary" 755
atomic_copy "$repository/assets/icon.svg" "$icon" 644
mkdir -p -- "$(dirname -- "$desktop")"
stage=$(mktemp "$desktop.tmp.XXXXXX")
temporary+=("$stage")
while IFS= read -r line; do
    case "$line" in
        Exec=*)
            if [[ "$binary" == *%* ]]; then
                # GIO checks the executable before expanding literal %% codes.
                # Keep the script fixed and pass the path as an argument; no
                # installation path or document name is evaluated as shell code.
                # Variables are expanded by the desktop-launched shell.
                # shellcheck disable=SC2016
                exec_script='exec "$0" "$@"'
                printf 'Exec=/bin/sh -c %s %s -- %%f\n' \
                    "$(desktop_argument "$exec_script")" "$(desktop_argument "$binary")"
            else
                printf 'Exec=%s -- %%f\n' "$(desktop_argument "$binary")"
            fi
            ;;
        Icon=*) printf 'Icon=%s\n' "$(desktop_string "$icon")" ;;
        *) printf '%s\n' "$line" ;;
    esac
done < "$repository/assets/pagewing.desktop" > "$stage"
chmod 644 "$stage"
mv -fT -- "$stage" "$desktop"
refresh_desktop_database
printf 'Installed %s\nDesktop entry: %s\n' "$binary" "$desktop"
