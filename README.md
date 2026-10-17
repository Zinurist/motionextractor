# Motion Extractor

Highlights the motion in a video by comparing each frame with an earlier one, based on Posy's
[Motion Extraction](https://youtu.be/NSS6yAMZF78) video.

## Run

Installers for Linux, Windows and macOS are attached to the GitHub releases. Linux packages need
ffmpeg installed; the other builds include it.

```sh
motion-extractor [video]                        # desktop app
motion-extractor serve --root ~/Videos          # web UI at http://127.0.0.1:8090/
motion-extractor render in.mp4 out.mp4 --offset 3f --color '#00ff88' --bloom
motion-extractor render --help                  # all options
```

## Build

Requires Rust, Node.js, ffmpeg + ffprobe on `PATH`, and the
[Tauri prerequisites](https://tauri.app/start/prerequisites/) for the desktop app.

```sh
npm --prefix web ci && npm --prefix web run build
cargo build --release                                  # target/release/motion-extractor
cargo build --release --no-default-features            # headless: serve/render only
cargo test --workspace                                 # Rust tests
npm --prefix e2e ci && npx --prefix e2e playwright install chromium && npm --prefix e2e test
```
