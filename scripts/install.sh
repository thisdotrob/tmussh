#!/bin/sh
set -eu

REPO="${TMUSSH_REPO:-thisdotrob/tmussh}"
VERSION="${TMUSSH_VERSION:-latest}"
INSTALL_DIR="${TMUSSH_INSTALL_DIR:-$HOME/.local/bin}"

fail() {
  printf 'tmussh install: %s\n' "$1" >&2
  exit 1
}

need() {
  command -v "$1" >/dev/null 2>&1 || fail "$1 is required"
}

target() {
  os=$(uname -s)
  arch=$(uname -m)

  case "$arch" in
    x86_64 | amd64) arch=x86_64 ;;
    arm64 | aarch64) arch=aarch64 ;;
    *) fail "unsupported architecture: $arch" ;;
  esac

  case "$os" in
    Linux) printf '%s-unknown-linux-gnu\n' "$arch" ;;
    Darwin) printf '%s-apple-darwin\n' "$arch" ;;
    *) fail "unsupported operating system: $os" ;;
  esac
}

download() {
  curl --fail --location --silent --show-error "$1" --output "$2"
}

verify_checksum() {
  checksum_file=$1
  archive=$2
  archive_name=$3

  if command -v sha256sum >/dev/null 2>&1; then
    checksum_line=$(awk -v file="$archive_name" '$2 == file { print $0 }' "$checksum_file")
    [ -n "$checksum_line" ] || fail "checksum not found for $archive_name"
    (cd "$(dirname "$archive")" && printf '%s\n' "$checksum_line" | sha256sum --check --status -) ||
      fail "checksum verification failed for $archive_name"
    return
  fi

  if command -v shasum >/dev/null 2>&1; then
    expected=$(awk -v file="$archive_name" '$2 == file { print $1 }' "$checksum_file")
    [ -n "$expected" ] || fail "checksum not found for $archive_name"
    actual=$(shasum -a 256 "$archive" | awk '{ print $1 }')
    [ "$expected" = "$actual" ] || fail "checksum verification failed for $archive_name"
    return
  fi

  fail "sha256sum or shasum is required"
}

need curl
need tar
need mktemp

target_triple=$(target)
archive_name="tmussh-$target_triple.tar.gz"

if [ "$VERSION" = "latest" ]; then
  release_base="https://github.com/$REPO/releases/latest/download"
else
  release_base="https://github.com/$REPO/releases/download/$VERSION"
fi

tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/tmussh.XXXXXXXXXX")
trap 'rm -rf "$tmpdir"' EXIT HUP INT TERM

archive="$tmpdir/$archive_name"
checksums="$tmpdir/SHA256SUMS"

download "$release_base/$archive_name" "$archive" ||
  fail "could not download $archive_name from $release_base"
download "$release_base/SHA256SUMS" "$checksums" ||
  fail "could not download SHA256SUMS from $release_base"

verify_checksum "$checksums" "$archive" "$archive_name"

tar -xzf "$archive" -C "$tmpdir"
[ -f "$tmpdir/tmussh" ] || fail "archive did not contain tmussh"

mkdir -p "$INSTALL_DIR"
if command -v install >/dev/null 2>&1; then
  install -m 0755 "$tmpdir/tmussh" "$INSTALL_DIR/tmussh"
else
  cp "$tmpdir/tmussh" "$INSTALL_DIR/tmussh"
  chmod 0755 "$INSTALL_DIR/tmussh"
fi

printf 'tmussh installed to %s\n' "$INSTALL_DIR/tmussh"
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) printf 'Add %s to PATH to run tmussh from any directory.\n' "$INSTALL_DIR" ;;
esac
