# Motion Extractor

Highlights the motion in a video by comparing each frame with an earlier one, based on Posy's
[Motion Extraction](https://youtu.be/NSS6yAMZF78) video.

> [!WARNING]
> **This project is vibe coded** — I wrote it largely with an AI coding assistant.
>
> I only use the **Linux CLI binary**. The **Windows and macOS builds** and the **`.deb`, `.rpm` and
> `.AppImage` packages** are built automatically, but I don't use them; they are provided for
> convenience only — **use them at your own risk**.

![Motion Extractor showing the motion of a Game of Life video as glowing cyan over the dimmed original](docs/screenshot.png)

## Run

Installers for Linux, Windows (x86_64 and arm64) and macOS (universal) are attached to the GitHub
releases, plus a portable Windows zip (unpack, run `motion-extractor.exe`); builds of the latest
`main` are in the rolling `nightly` pre-release. Linux packages need ffmpeg installed; the other
builds include it.

```sh
motion-extractor [video]                        # desktop app
motion-extractor serve --root ~/Videos          # web UI at http://127.0.0.1:8090/
motion-extractor render in.mp4 out.mp4 --offset 3f --color '#00ff88' --bloom
motion-extractor render --help                  # all options
```

## Build

### Dependencies

- [mise](https://mise.jdx.dev): installs the pinned Node.js and Rust (`mise install`).
- ffmpeg and ffprobe on `PATH` (with libx264, libx265 and libvpx for all render presets).
- A C compiler and `pkg-config`.
- **Desktop app only:** the system webview and GTK libraries (Linux), see the table. The headless
  build (`serve`, `render`, `probe`) does not need them.

| System | Packages |
|---|---|
| Fedora | `sudo dnf install ffmpeg gcc pkgconf-pkg-config` (ffmpeg from RPM Fusion), desktop: `webkit2gtk4.1-devel gtk3-devel librsvg2-devel libxdo-devel dbus-devel libappindicator-gtk3-devel gstreamer1-plugins-good` |
| Debian / Ubuntu | `sudo apt install ffmpeg build-essential pkg-config`, desktop: `libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libxdo-dev libdbus-1-dev libayatana-appindicator3-dev gstreamer1.0-plugins-good` |
| Arch | `sudo pacman -S ffmpeg base-devel`, desktop: `webkit2gtk-4.1 gtk3 librsvg xdotool dbus libappindicator-gtk3 gst-plugins-good` |
| macOS | `xcode-select --install`, `brew install ffmpeg` |
| Windows | [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) ("Desktop development with C++"), ffmpeg (e.g. `choco install ffmpeg`); WebView2 ships with Windows 10/11 |

### Commands

```sh
mise install              # Node.js, Rust, actionlint
mise run build            # target/release/motion-extractor (desktop app + CLI)
mise run build:headless   # the same without the desktop window: no GTK/webview libraries needed
mise run serve            # web UI at http://127.0.0.1:8090/ for videos in the repository directory
mise run serve -- --root ~/Videos   # … for videos in ~/Videos (arguments after --)
mise run test             # tests (mise tasks lists everything)
```

`scripts/quick-deploy.sh user@host` builds the web UI server for a Linux host and runs it there as a
systemd user service on port 8090. `serve` has no authentication — on a network, put it behind an
authenticating reverse proxy.
