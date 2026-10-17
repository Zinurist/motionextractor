# Motion Extractor

Highlights the motion in a video by comparing each frame with an earlier one, based on Posy's
[Motion Extraction](https://youtu.be/NSS6yAMZF78) video.

## Build & run

Requires Rust, and ffmpeg + ffprobe on `PATH`.

```sh
cargo build --release
./target/release/motion-extractor render input.mp4 output.mp4 --offset 3f
cargo test --workspace
```
