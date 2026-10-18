# Motion Extractor

Highlights the motion in a video by comparing each frame with an earlier one, based on Posy's
[Motion Extraction](https://youtu.be/NSS6yAMZF78) video.

> [!WARNING]
> **This project is vibe coded** — I wrote it largely with an AI coding assistant.
>
> I only use the **Linux CLI binary** (`motion-extractor-cli-…-x86_64-unknown-linux-gnu`) in `serve`
> mode. The **Windows and macOS builds** and the **`.deb`, `.rpm` and `.AppImage` packages** are
> built automatically (Windows and macOS also run the automated test suite), but I don't use them;
> they are provided for convenience only — **use them at your own risk**.

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

Requires [mise](https://mise.jdx.dev), ffmpeg + ffprobe on `PATH`, and the
[Tauri prerequisites](https://tauri.app/start/prerequisites/) for the desktop app.

```sh
mise install              # Node.js, Rust, actionlint
mise run build            # target/release/motion-extractor
mise run serve            # web UI at http://127.0.0.1:8090/
mise run test             # tests (mise tasks lists everything)
```
