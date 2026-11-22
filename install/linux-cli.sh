#!/usr/bin/env bash
# Install the Rust CLIs from this checkout; service provisioning is separate.
set -euo pipefail

usage() {
  cat <<'HELP'
Usage: install/linux-cli.sh [--root ABSOLUTE_DIR]

Build and install cg and cg-registry from this checkout on Linux.
Requires: Rust/Cargo. Default root: CARGO_INSTALL_ROOT, CARGO_HOME, or $HOME/.cargo.
Effects: replaces these two executables in ROOT/bin; Cargo may download dependencies.
Does not start services or change PATH. Exit 0: installed/help; 1: failed; 2: usage error.
HELP
}

install_root="${CARGO_INSTALL_ROOT:-${CARGO_HOME:-}}"
while (($#)); do
  case "$1" in
    --help|-h) usage; exit 0 ;;
    --root)
      if (($# < 2)) || [[ -z "$2" || "$2" == --* ]]; then
        echo '--root requires an absolute directory; see --help.' >&2
        exit 2
      fi
      install_root="$2"
      shift 2
      ;;
    *) echo 'Unknown installation option; see --help.' >&2; exit 2 ;;
  esac
done
if [[ "$(uname -s)" != Linux ]]; then
  echo 'This installer requires Linux.' >&2
  exit 1
fi
if [[ -z "$install_root" ]]; then
  : "${HOME:?HOME, CARGO_HOME or CARGO_INSTALL_ROOT is required}"
  install_root="$HOME/.cargo"
fi
if [[ "$install_root" != /* ]]; then
  echo 'The installation root must be an absolute path.' >&2
  exit 2
fi
if ! command -v cargo >/dev/null 2>&1; then
  echo 'Cargo is required. Install a Rust toolchain first.' >&2
  exit 1
fi
repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
cargo install --path "$repo_root/crates/gateway-daemon" \
  --bin cg --bin cg-registry --locked --force --root "$install_root"
for binary in cg cg-registry; do
  if [[ ! -x "$install_root/bin/$binary" ]]; then
    echo "Installation did not create $install_root/bin/$binary" >&2
    exit 1
  fi
done
echo "Installed cg and cg-registry in $install_root/bin"
if [[ ":${PATH:-}:" != *":$install_root/bin:"* ]]; then
  echo "Add $install_root/bin to PATH to run them by name."
fi
