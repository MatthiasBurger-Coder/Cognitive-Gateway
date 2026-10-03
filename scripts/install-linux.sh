#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/install-linux.sh [--root DIR] [--with-postgres]

Install the cg and cg-registry CLIs from this checkout on Linux.
The default root is CARGO_INSTALL_ROOT, CARGO_HOME, or $HOME/.cargo.
The executables are installed into ROOT/bin.
--with-postgres also starts the persistent PostgreSQL Compose service.
EOF
}

if [[ "$(uname -s)" != Linux ]]; then
  echo 'This installer requires Linux.' >&2
  exit 1
fi

install_root="${CARGO_INSTALL_ROOT:-${CARGO_HOME:-}}"
with_postgres=false
if [[ -z "$install_root" ]]; then
  if [[ -z "${HOME:-}" ]]; then
    echo 'HOME, CARGO_HOME, or CARGO_INSTALL_ROOT is required.' >&2
    exit 1
  fi
  install_root="$HOME/.cargo"
fi
while (($#)); do
  case "$1" in
    --root)
      if (($# < 2)) || [[ -z "$2" ]]; then
        echo '--root requires a directory.' >&2
        exit 2
      fi
      install_root="$2"
      shift 2
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    --with-postgres)
      with_postgres=true
      shift
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ -z "$install_root" || "$install_root" != /* ]]; then
  echo 'The installation root must be an absolute path.' >&2
  exit 2
fi
if ! command -v cargo >/dev/null 2>&1; then
  echo 'Cargo is required. Install a Rust toolchain with Cargo first.' >&2
  exit 1
fi
if "$with_postgres" && { ! command -v docker >/dev/null 2>&1 || ! docker compose version >/dev/null 2>&1; }; then
  echo 'Docker with the Compose plugin is required for --with-postgres.' >&2
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
if "$with_postgres"; then
  "$repo_root/scripts/start-postgres.sh"
fi
if [[ ":$PATH:" != *":$install_root/bin:"* ]]; then
  echo "Add $install_root/bin to PATH to run them by name."
fi
