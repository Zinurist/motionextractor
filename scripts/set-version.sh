#!/usr/bin/env bash
# Sets the app version X.Y everywhere (Cargo workspace, web and e2e packages and their lockfiles).
# Cargo and npm need three numbers, so the files say X.Y.0; tags and releases use vX.Y.
#
#   scripts/set-version.sh X.Y
set -euo pipefail

version=${1:?usage: $0 X.Y}
if ! [[ $version =~ ^[0-9]+\.[0-9]+$ ]]; then
  echo "error: the version must be X.Y (e.g. 1.4), got: $version" >&2
  exit 1
fi
cd "$(dirname "$0")/.."

sed -i.bak "0,/^version = \".*\"/s//version = \"$version.0\"/" Cargo.toml && rm Cargo.toml.bak
cargo update --workspace --offline --quiet
for pkg in web e2e; do
  npm --prefix "$pkg" version "$version.0" --no-git-tag-version --allow-same-version >/dev/null
done
echo "Version set to $version. Commit, then tag v$version on main after the PR is merged."
