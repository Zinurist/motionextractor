#!/usr/bin/env bash
# Downloads static GPL ffmpeg/ffprobe builds for bundling as Tauri sidecars.
#
#   scripts/fetch-ffmpeg.sh <rust-target> <output-dir>
#
# Writes <output-dir>/ffmpeg-<target>[.exe] and ffprobe-<target>[.exe]. Sources:
#   Linux, Windows: https://github.com/BtbN/FFmpeg-Builds (latest release build)
#   macOS:          https://ffmpeg.martin-riedl.de (latest release build, SHA-256 checked)
set -euo pipefail

target=${1:?rust target}
out=${2:?output dir}
mkdir -p "$out"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

btbn() {
  # Newest versioned release asset for a platform, e.g. ffmpeg-n8.1-latest-linux64-gpl-8.1.tar.xz
  local platform=$1 ext=$2
  curl -fsSL https://api.github.com/repos/BtbN/FFmpeg-Builds/releases/latest |
    jq -r --arg p "$platform" --arg e "$ext" \
      '.assets[] | select(.name | test("^ffmpeg-n[0-9.]+-latest-" + $p + "-gpl-[0-9.]+\\." + $e + "$")) | .browser_download_url' |
    sort -V | tail -n 1
}

riedl() {
  # Static macOS build of one tool for one architecture (arm64 | amd64): the newest release
  # (not snapshot) build listed on the site, checked against its published SHA-256.
  local arch=$1 tool=$2 dest=$3 base=https://ffmpeg.martin-riedl.de
  local path
  path=$(curl -fsSL "$base/" |
    grep -oE "/download/macos/$arch/[0-9]+_[0-9.]+/$tool\.zip" | sort -t_ -k2 -V | tail -n 1)
  [ -n "$path" ] || { echo "no macOS $arch build of $tool found" >&2; exit 1; }
  curl -fsSL -o "$tmp/$tool-$arch.zip" "$base$path"
  local want
  want=$(curl -fsSL "$base$path.sha256" | grep -oE '^[0-9a-f]{64}')
  echo "$want  $tmp/$tool-$arch.zip" | shasum -a 256 -c -
  unzip -o -q "$tmp/$tool-$arch.zip" -d "$tmp/$arch"
  cp "$tmp/$arch/$tool" "$dest"
}

case "$target" in
  x86_64-unknown-linux-gnu)
    url=$(btbn linux64 'tar\.xz')
    curl -fsSL -o "$tmp/ff.tar.xz" "$url"
    tar -xJf "$tmp/ff.tar.xz" -C "$tmp"
    for tool in ffmpeg ffprobe; do cp "$tmp"/ffmpeg-*/bin/$tool "$out/$tool-$target"; done
    ;;
  x86_64-pc-windows-msvc)
    url=$(btbn win64 zip)
    curl -fsSL -o "$tmp/ff.zip" "$url"
    unzip -q "$tmp/ff.zip" -d "$tmp"
    for tool in ffmpeg ffprobe; do cp "$tmp"/ffmpeg-*/bin/$tool.exe "$out/$tool-$target.exe"; done
    ;;
  aarch64-apple-darwin | x86_64-apple-darwin)
    arch=$([ "$target" = aarch64-apple-darwin ] && echo arm64 || echo amd64)
    for tool in ffmpeg ffprobe; do riedl "$arch" "$tool" "$out/$tool-$target"; done
    ;;
  universal-apple-darwin)
    for tool in ffmpeg ffprobe; do
      riedl arm64 "$tool" "$tmp/$tool-arm64"
      riedl amd64 "$tool" "$tmp/$tool-amd64"
      lipo -create -output "$out/$tool-$target" "$tmp/$tool-arm64" "$tmp/$tool-amd64"
    done
    ;;
  *)
    echo "unsupported target: $target" >&2
    exit 1
    ;;
esac

chmod +x "$out"/ff*
ls -l "$out"
