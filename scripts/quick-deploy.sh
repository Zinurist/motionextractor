#!/usr/bin/env bash
# Builds Motion Extractor for a Linux host and deploys it there as a systemd *user* service
# ("serve" mode on 0.0.0.0, no authentication — put a reverse proxy in front if you need any).
#
#   scripts/quick-deploy.sh HOST
#
# HOST is anything ssh accepts (user@host, an alias from ~/.ssh/config, …). Options via environment:
#   MX_PORT   port to listen on            (default 8090)
#   MX_ROOT   media directory on the host  (default ~/motion-extractor; created if missing)
#
# The binary is cross-compiled for the host's CPU architecture and glibc version with
# cargo-zigbuild (provided by mise; without mise, zig and cargo-zigbuild must be on PATH).
#
# On the host this installs:
#   ~/.local/bin/motion-extractor                       the binary (replaced on every deploy)
#   ~/.config/systemd/user/motion-extractor.service     the service (only created once)
# The host needs ffmpeg/ffprobe on PATH.
set -euo pipefail

host=${1:?usage: $0 HOST}
port=${MX_PORT:-8090}
root=${MX_ROOT:-'~/motion-extractor'}
service=motion-extractor
repo=$(cd "$(dirname "$0")/.." && pwd)

# One shared SSH connection for all steps (a single password/passphrase prompt at most).
control=$(mktemp -u "${TMPDIR:-/tmp}/mx-deploy-ssh.XXXXXX")
ssh_opts=(-o ControlMaster=auto -o "ControlPath=$control" -o ControlPersist=120)
# Commands are expanded locally on purpose (values like $service are sent to the host).
# shellcheck disable=SC2029
remote() { ssh "${ssh_opts[@]}" "$host" "$@"; }
trap 'ssh "${ssh_opts[@]}" -O exit "$host" 2>/dev/null || true' EXIT

echo "==> Checking $host"
case $(remote uname -m) in
  x86_64 | amd64) target=x86_64-unknown-linux-gnu ;;
  aarch64 | arm64) target=aarch64-unknown-linux-gnu ;;
  *) echo "error: unsupported architecture on $host: $(remote uname -m)" >&2; exit 1 ;;
esac
glibc=$(remote getconf GNU_LIBC_VERSION | awk '{print $2}')
if [ -z "$glibc" ]; then
  echo "error: could not determine the glibc version on $host (only glibc-based Linux is supported)" >&2
  exit 1
fi
if ! remote 'command -v ffmpeg >/dev/null && command -v ffprobe >/dev/null'; then
  echo "warning: ffmpeg/ffprobe not found on $host; install them before opening videos" >&2
fi

echo "==> Building for $target (glibc $glibc)"
cd "$repo"
build=(rustup target add "$target" '&&' cargo zigbuild --release -p motion-extractor
  --no-default-features --target "$target.$glibc")
if command -v mise >/dev/null; then
  mise run web:build
  mise exec zig@0.14 cargo:cargo-zigbuild -- bash -c "${build[*]}"
else
  npm ci --prefix web && npm --prefix web run build
  bash -c "${build[*]}"
fi
binary=$repo/target/$target/release/motion-extractor

echo "==> Uploading the binary"
remote 'mkdir -p ~/.local/bin'
scp -q "${ssh_opts[@]}" "$binary" "$host:.local/bin/$service.new"
# Rename over the old file: replacing a running binary in place fails ("text file busy").
remote "chmod +x ~/.local/bin/$service.new && mv -f ~/.local/bin/$service.new ~/.local/bin/$service"

echo "==> Setting up the service (if needed)"
remote bash -s -- "$service" "$port" "$root" <<'REMOTE'
set -euo pipefail
service=$1 port=$2 root=$3
root=${root/#\~/$HOME}
mkdir -p "$root" ~/.config/systemd/user
unit=~/.config/systemd/user/$service.service
if [ ! -f "$unit" ]; then
  cat > "$unit" <<UNIT
[Unit]
Description=Motion Extractor (web UI)
After=network-online.target

[Service]
ExecStart=%h/.local/bin/$service serve --bind 0.0.0.0 --port $port --root "$root"
Restart=on-failure

[Install]
WantedBy=default.target
UNIT
  echo "created $unit"
fi
systemctl --user daemon-reload
systemctl --user enable "$service" >/dev/null
# Keep the service running when the user is not logged in.
if [ "$(loginctl show-user "$USER" --property=Linger --value 2>/dev/null)" != yes ]; then
  loginctl enable-linger "$USER" 2>/dev/null ||
    echo "warning: could not enable lingering; the service stops when $USER logs out (ask an admin: loginctl enable-linger $USER)" >&2
fi
REMOTE

echo "==> Restarting"
remote "systemctl --user restart $service"
sleep 2
if ! remote "systemctl --user is-active --quiet $service"; then
  echo "error: the service did not start; recent log:" >&2
  remote "journalctl --user -u $service -n 30 --no-pager" >&2 || true
  exit 1
fi

echo
echo "Motion Extractor is running: http://${host#*@}:$port/"
echo "(Open port $port in the host's firewall if needed, e.g. firewall-cmd --add-port=$port/tcp --permanent && firewall-cmd --reload.)"
