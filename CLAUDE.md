# Motion Extractor — Design & Specification

> **This file is the single source of truth for the design and specification of the project.**
> It MUST be kept up to date: any change to features, architecture, APIs, settings, file layout,
> build/test commands or conventions must be reflected here in the same change.
> Keep it free of anything specific to a particular developer machine — this is an open source project.

Status: **M1–M6 done** (effect pipeline, ffmpeg integration, CLI, preview engine, HTTP/WebSocket
server, web UI, E2E suite, desktop app, CI/CD). Sections describe
the target state; the [Roadmap](#9-roadmap) tracks what is implemented.

---

## 1. Purpose

A cross-platform desktop application (Linux, Windows and macOS) that performs **motion extraction** on
videos, as popularised by Posy's video [Motion Extraction](https://youtu.be/NSS6yAMZF78)
(tutorial for video editors: [Motion Extraction tutorial](https://youtu.be/woj4vfMLpao)).

For every frame `t` of a video, the app computes the difference against another frame (by default
the frame `k` frames earlier). Static parts of the image cancel out to black; only the parts that
changed remain bright. The result can be tinted, bloomed, overlaid on the original video, previewed
interactively with fast scrubbing, and rendered to a video file via ffmpeg.

### Goals

1. Interactive preview: scrubbing and parameter changes must feel instant.
2. Preview and final render produce the same image (up to resolution).
3. Sensible defaults everywhere; power-user knobs (ffmpeg options) available but optional.
4. Fully tested: unit, integration and end-to-end tests run in CI on every push.
5. Same UI in desktop mode and in a headless server mode.

### Non-goals (for now)

- Video editing (cutting, multiple clips, timelines with tracks).
- Real-time camera input.

---

## 2. Features (functional specification)

### 2.1 Input

- Open any video ffmpeg can decode. Opening via: native file dialog (desktop), drag & drop onto the
  window (desktop), server-side file browser (server mode), path given on the command line.
- **Server mode: upload and download.** The file browser's "Upload videos…" sends one or more local
  files (any type — no file-type filter, so e.g. upper-case `.MOV` files from phones are not
  hidden) to the directory being browsed, one after another with progress; a single upload is
  opened right away, after several the listing stays open to pick one. Names that already exist get
  a " (2)" suffix instead of overwriting. The browser shows the free space of the directory's
  filesystem. Files in the browser and finished renders (render dialog)
  can be downloaded. Both stay within the `--root` directories.
- The video is probed with `ffprobe`: resolution, frame rate, frame count, duration, audio presence.
- Frames are addressed by **index** `0..N-1`. The video is treated as constant frame rate (CFR) at
  its probed average frame rate; variable-frame-rate input is normalised to CFR on decode
  (ffmpeg `fps` filter), so frame index ↔ time is always `time = index / fps`.
- One video is open per session.

### 2.2 Comparison modes

| Mode | Reference frame for frame `t` | Meaning |
|---|---|---|
| **Rolling offset** (default) | `t − k` | Motion: what moved during the last `k` frames |
| **Fixed reference** | a user-chosen frame `r` | Change: what differs from frame `r` |

- **Offset `k`** can be entered **in frames or in seconds** (unit toggle). Seconds are converted with
  `k = round(seconds × fps)`; the UI always shows both representations. `k` is a non-zero integer;
  negative values compare against a *future* frame. Default: `k = 1` frame.
- If the reference index is outside `0..N-1` (e.g. the first `k` frames), the frame has
  **no reference** and the motion mask is zero (output = background only).
- **Fixed reference frame** can be set from the current playhead ("Set reference to current frame")
  or by entering a frame index / timestamp. The reference frame is shown as a marker on the timeline.

### 2.3 Effect pipeline

All effect parameters are **resolution independent** (spatial sizes are fractions of frame height) so
a low-resolution preview matches the full-resolution render. The pipeline is a pure function
`process(current, reference, params) -> output` implemented once in the `core` crate and used by
both preview and render.

Per pixel (values normalised to `0..1`, sRGB-encoded):

1. **Pre-blur (denoise)**, optional: Gaussian blur of both inputs with radius `denoise` (fraction of
   height, default `0` = off). Suppresses sensor and compression noise. Only the differencing uses
   the blurred frames; the overlay background stays sharp. Because it depends only on the frame,
   callers prepare each frame once (`core::Prepared`) and reuse it as current and as reference.
2. **Difference** `D = |C − R|` per channel.
3. **Motion mask** `m = metric(D)` where `metric` is `luma` (Rec. 709 weights, default) or `max`
   (max of channels).
4. **Threshold & gain** `m' = clamp((m − threshold) × gain, 0, 1)`; optional `gamma` applied as
   `m'^(1/gamma)`. Defaults: `threshold = 0.02`, `gain = 4.0`, `gamma = 1.0`.
5. **Fill** of the motion layer (`motion_fill`): `color` (default; `F = color`, chosen with a color
   picker, default white) or `original` (`F = C`: the moving parts show the original video, as if seen
   through a window — the "solid background, motion shows the video" look from the video).
6. **Bloom**, optional (default off): extract `L` above `bloom_threshold`, blur with radius
   `bloom_radius` (multi-scale downsample/blur/upsample), add `bloom_intensity ×` blurred result.
   Bloom is computed in linear light and converted back to sRGB. With a color fill the blurred
   mask is tinted with `color`; with the original fill the masked original itself is blurred, so it
   glows in the video's colours.
7. **Composite** over the background:
   - Background **solid color** (default; `background_color`, default black):
     `out = lerp(background_color, F, m') + bloom`.
   - Background **original video** (overlay mode): `bg = C × background_brightness` (default `1.0`,
     lower values dim the original so the motion pops), then
     `out = lerp(bg, F, m' × overlay_opacity) + bloom` — only motion is drawn on top of the original.
     (Original background with original fill would just be the video; the UI does not offer it.)
   - `"background": "black"` from older presets is read as `"color"`.
   - Output is clamped to `0..1`.

Optional, lower priority:

- **Classic mode** (the original editor technique from the video): `out = 0.5·C + 0.5·(1 − R)`,
  producing a grey image with motion as deviations. Bypasses steps 2–7 (uses the denoised frames).
  Implemented in `core` and the CLI (`--classic`).
- **Alpha output**: render the motion layer with `m'` (+bloom) as alpha channel for compositing in
  other tools (requires an alpha-capable codec: ProRes 4444, VP9/WebM, PNG sequence).

### 2.4 Preview & scrubbing

- A viewer shows the processed frame at the playhead. Timeline with scrubbing, play/pause, frame
  step (±1), jump to start/end, keyboard shortcuts (Space, ←/→, Shift+←/→ = ±1 s, Home/End).
- **Every parameter change updates the viewer immediately** (no "apply" button). Sliders that
  differ from their default show a reset button (circular arrow).
- **Trim**: a window over the timeline whose edges can be dragged (the playhead follows the edge
  being dragged to show the cut frame); outside it the timeline is dimmed. I / O set the in / out
  point at the playhead; the ✕ next to the trim summary removes it. Playback starts at the trim
  start (if the playhead is outside the trim or at its end) and stops at the trim end; Home/End and
  the ⏮/⏭ buttons jump to the trim bounds. Scrubbing outside the trim stays possible. The trim *is*
  the render range (`render.start`/`render.end`), so the render contains exactly the trimmed part;
  opening another video clears it.
- View modes: *Output*, *Original*, *Split* (draggable divider), *Side by side*.
- Zoom (fit / 100% / mouse-wheel) and pan.
- **Preview resolution** setting (e.g. 360p / 540p / 720p / 1080p / native, default 540p).
- The timeline visualises which frames are currently cached (like video editors do).
- Playback at the video's frame rate, **with the video's audio**: mute toggle and a volume slider
  from 0 to 200 % (double-click: 100 %), both remembered per browser. Volumes above 100 % go through
  a Web Audio gain node (created on the first play, as browsers require a user gesture); without
  Web Audio the element volume is used (max. 100 %). The clock follows the audio while it plays (wall clock without audio, or after the
  audio ends). Scrubbing is silent.
- **Buffering instead of freezing**: if the shown frame trails the playhead by more than 3 frames
  (decoding or processing can't keep up), playback pauses the clock and the audio and shows
  "Buffering…" until the current frame is shown and the next second of frames (bounded by the trim
  end and half the cache capacity) is decoded, then continues.
- Transport icons follow media conventions: ⏮/⏭ jump to the trim bounds, |◀/▶| step one frame,
  ▶ plays.

See [§4 Preview engine](#4-preview-engine) for how this is made fast.

### 2.5 Rendering (export)

- Renders the processed video at full (or chosen) resolution through ffmpeg.
- **Presets** with reasonable defaults:
  - *H.264 MP4* (default): `libx264`, `-crf 18`, `-preset medium`, `yuv420p`, `+faststart`
  - *H.265 MP4*: `libx265`, `-crf 20`
  - *VP9 WebM*, *ProRes MOV* (incl. 4444 for alpha), *FFV1 MKV* (lossless), *PNG sequence*
- Editable options: codec, quality (CRF/bitrate), encoder preset, pixel format, output scale, time
  range (in/out points), audio (*encode* — default: original audio re-encoded to AAC, Opus for
  WebM, trimmed to the range; *copy* — stream copy; *none*), and a free-form **extra ffmpeg
  arguments** field for power users.
- Without an explicit codec, the codec follows the output extension (`.webm` → VP9, `.mov` →
  ProRes, `.png`/`%` pattern → PNG sequence, anything else → H.264).
- PNG sequences: the output is a directory (frames named `frame_%06d.png`) or a `%` pattern.
- The UI shows the exact ffmpeg command that will be run.
- Rendering runs in the background with progress (frames, fps, ETA) and can be cancelled. The
  preview stays usable while rendering.
- Output path selection via native save dialog (desktop) or path field (server mode). Existing files
  are not overwritten without confirmation.

### 2.6 Settings (persisted)

Stored as JSON in the OS-standard per-user config directory (`<config dir>/motion-extractor/settings.json`,
overridable with `--settings`). Missing keys use defaults.

| Setting | Default | Notes |
|---|---|---|
| `cache_max_memory_mb` | 2048 | Upper bound for the preview frame cache (64 – 1048576). UI shows how many frames / seconds fit. |
| `preview_height` | 540 | Height of cached preview frames (aspect preserved, never above native); 0 = native. |
| `decode_workers` | 2 | Parallel ffmpeg decoder processes filling the cache (1 – 16). |
| `hwaccel` | false | `-hwaccel auto` for preview decoding. |
| `jpeg_quality` | 85 | Quality of preview frames sent to non-loopback clients. |
| `ffmpeg_path` / `ffprobe_path` | null | Custom executables (applied at startup); null = auto (env var, bundled, `PATH`). |

Changing `preview_height`, `decode_workers` or `hwaccel` reopens the video (cache refilled).
The current preset (comparison, effect, render options) is session state, not a setting.

Effect parameters + render options of a session can be saved/loaded as a **preset file** (JSON).

### 2.7 Command line

Single binary `motion-extractor`:

```
motion-extractor [FILE]                     # start desktop GUI (default)
motion-extractor serve [--bind ADDR] [--port N] [--root DIR]... [--settings FILE] [FILE]
motion-extractor render IN OUT [effect/render options | --preset FILE]
motion-extractor probe FILE                 # video info as JSON
```

`serve` defaults: bind `127.0.0.1`, port 8090 (0 = any free port), root = current directory. It
prints `Motion Extractor listening on http://HOST:PORT/` as its first stdout line (the E2E
harness parses it). `serve` has no authentication; anyone who needs it puts an authenticating reverse
proxy in front.

`render` makes every feature scriptable and is used heavily by integration tests. Options
(`motion-extractor render --help` is authoritative):

- Comparison: `--offset 3|3f|0.5s|-2` (rolling), `--reference 120|2.5s|1:02.5` (fixed).
- Effect: `--metric luma|max`, `--threshold`, `--gain`, `--gamma`, `--denoise`, `--color #rrggbb`,
  `--motion-fill color|original`, `--background-color #rrggbb`,
  `--bloom` (+ `--bloom-radius`, `--bloom-intensity`, `--bloom-threshold`), `--overlay`
  (+ `--background-brightness`, `--overlay-opacity`), `--classic`.
- Output: `--codec h264|h265|vp9|prores|ffv1|png`, `--crf`, `--encoder-preset`, `--pix-fmt`,
  `--height`, `--start`/`--end` (TIME, end exclusive), `--audio encode|copy|none`,
  `--ffmpeg-arg=ARG` (repeatable).
- `--preset FILE` loads settings (flags override it), `--save-preset FILE` writes the effective
  settings, `--dry-run` prints the encoder command, `-y` overwrites, `-q` silences progress.
- TIME values: frame index (`120`, `120f`), seconds (`2.5s`) or timestamp (`1:02.5`).

**Preset file** (JSON, all sections optional, missing values use defaults):

```json
{
  "version": 1,
  "comparison": { "mode": "rolling", "offset": { "value": 0.5, "unit": "seconds" } },
  "effect": { "blend": "difference", "metric": "luma", "denoise": 0.0, "threshold": 0.02,
              "gain": 4.0, "gamma": 1.0, "color": "#ffffff", "motion_fill": "color",
              "bloom": { "enabled": false, "threshold": 0.0, "radius": 0.02, "intensity": 1.0 },
              "background": "color", "background_color": "#000000",
              "background_brightness": 1.0, "overlay_opacity": 1.0 },
  "render": { "codec": "h264", "crf": null, "encoder_preset": null, "pix_fmt": null,
              "audio": "encode", "extra_args": [], "start": null, "end": null, "height": null }
}
```

Fixed comparison: `{ "mode": "fixed", "frame": 120 }`. The same JSON shapes are used by the HTTP API.

---

## 3. Architecture

### 3.1 Decision: web UI served by a local Rust backend, wrapped in a native webview

```
┌─────────────────────── motion-extractor process ────────────────────────┐
│                                                                          │
│  Desktop shell (Tauri)            HTTP + WebSocket server (axum)         │
│  ┌──────────────────────┐  load   ┌───────────────────────────────────┐  │
│  │ native window +      │ ──────▶ │ static UI (embedded) · REST API · │  │
│  │ system webview       │ 127.0.0.1│ WS: frames + events               │  │
│  └──────────────────────┘         └──────────────┬────────────────────┘  │
│                                                  │                       │
│                         Engine: session, frame cache, prefetch, render   │
│                                                  │                       │
│                         core (effect)    media (ffmpeg/ffprobe procs)    │
└──────────────────────────────────────────────────┼───────────────────────┘
                                                   ▼
                                          ffmpeg / ffprobe
```

- **Backend (Rust)** owns everything heavy: decoding, the frame cache, the effect, rendering.
- **Frontend** is a web app (Svelte + TypeScript + Vite) that only displays processed frames and
  edits parameters. It contains no image processing.
- **Desktop mode**: the binary starts the server on `127.0.0.1` with a random port and a random
  session token, then opens a Tauri window (system webview: WebKitGTK on Linux, WebView2 on Windows, WKWebView on macOS)
  pointing at it. Native dialogs (`rfd`, XDG portal on Linux) and drag & drop are handled in the
  backend process and pushed to the UI over the WebSocket.
- **Server mode** (`serve`): same server, no window. Any browser can connect. Used for E2E tests and
  for running on a remote/headless machine (the host keeps the cache and does the work).

**Rationale.** One UI codebase for desktop and server mode; E2E tests drive the real UI with
Playwright against server mode, exercising the exact code path of the desktop app. The concern that a
web UI is too slow for preview is addressed by keeping all processing in the backend and shipping
finished frames: on loopback, a raw 960×540 RGB frame (1.5 MB) transfers in well under a
millisecond, so the viewer is limited by processing, not transport (see §4). Alternatives considered:
a native immediate-mode GUI (egui) — fast, but E2E testing and a server mode would need a second UI;
WebGL-side processing — fast, but duplicates the effect in a shader and risks preview/render
mismatch.

**Security.** The server binds to loopback by default. On a loopback address every request must carry a
matching `Host` header (and `Origin`, if present), preventing other web pages / DNS rebinding from
driving the app. The desktop app additionally uses a random session token (query param on first
load → `HttpOnly` cookie; checked on all routes). Server mode has **no authentication**: bound to a
non-loopback address, anyone who can reach the port can use it (put an authenticating reverse proxy
in front if needed). In server mode the server-side file browser and all input/output paths are
restricted to `--root` directories (default: the current working directory).

### 3.2 Repository layout

```
Cargo.toml                 workspace
crates/
  core/    (mx-core)       pure effect pipeline, frame types, params (no IO, no async)
  media/   (mx-media)      ffprobe/ffmpeg subprocess wrappers: probe, decode stream, encode sink;
                           feature `testing` = lavfi fixture generators shared by all tests
  engine/  (mx-engine)     render jobs, presets; later session state, frame cache, prefetch
  server/  (mx-server)     axum HTTP + WebSocket API, embeds built frontend (rust-embed)
  app/     (motion-extractor)  the binary: CLI, desktop shell (Tauri, feature `desktop`, default
                           on), `serve`, `render`, `probe`
web/                       Svelte 5 + TypeScript + Vite frontend (built into crates/server)
e2e/                       Playwright tests (+ fixture video generation)
.github/workflows/         CI and release pipelines
```

Building `app` without the `desktop` feature yields a headless binary (no webview dependencies) that
supports `serve`, `render` and `probe`.

### 3.2.1 Desktop shell (`crates/app/src/desktop.rs`, feature `desktop`)

- `motion-extractor [FILE]` without a subcommand starts the desktop app: the server on
  `127.0.0.1:<random port>` with a random token in desktop mode, then a Tauri window loading
  `http://127.0.0.1:<port>/?token=…` (`WebviewUrl::External`; no Tauri IPC is used, the UI talks to
  the server like any browser).
- Native dialogs come from `tauri-plugin-dialog` (blocking calls from server worker threads; the
  plugin runs them on the UI thread). Files dropped onto the window are opened via the session;
  failures become an `error` event (toast).
- `--smoke-test` (hidden) exits 0 as soon as the UI's JavaScript connects to the WebSocket
  (`ServerConfig::on_connect`), 1 after 60 s — used by CI on every OS.
- Windows: the binary uses the GUI subsystem (no console window); CLI subcommands attach to the
  parent console (`AttachConsole`).
- Configuration: `crates/app/tauri.conf.json` (identifier `io.github.motionextractor`, icons in
  `crates/app/icons/` generated from `icon.svg` with `tauri icon`). Linux packages depend on the
  system ffmpeg (`.deb`: `ffmpeg`, `.rpm`: `/usr/bin/ffmpeg`). `tauri.ffmpeg.conf.json` is a
  config overlay used by release builds to bundle `binaries/ffmpeg` and `binaries/ffprobe` as
  sidecars (placed next to the executable, where `Ffmpeg::locate` finds them).
- Tauri CLI: `web/node_modules/.bin/tauri` (dev dependency of `web`); run it from `crates/app`.

### 3.3 ffmpeg integration

- ffmpeg/ffprobe are used as **external processes** (no libav linking): simple cross-platform
  builds, no ABI issues, and users can swap in their own ffmpeg build.
- Executables are found via `MOTION_EXTRACTOR_FFMPEG` / `MOTION_EXTRACTOR_FFPROBE`, then next to the
  running executable (bundled), then `PATH`.
- **Probe**: `ffprobe -show_streams -show_format` JSON. Frame rate = `avg_frame_rate`; frame count =
  `nb_frames` when the stream is CFR, else `round(duration × fps)` with the duration taken from the
  stream, then its Matroska `DURATION` tag, then the container (which may include longer audio);
  rotation metadata swaps the size. Attached pictures (cover art) are not video streams. Tested
  with ffmpeg 6.0 and 8.1.
- **Decode**: `ffmpeg [-hwaccel auto] [-ss <seek>] -i IN -copyts -an -sn -dn
  -vf "fps=<fps>[,trim=start=<abs>][,scale=W:H:flags=area]" [-frames:v N] -f rawvideo -pix_fmt rgb24 pipe:1`.
  Frame-accurate seeking to frame `s`:
  - target time `(s − 0.5) / fps` (half a frame early, so millisecond-rounded timestamps such as
    Matroska's can never skip the frame);
  - `-copyts` keeps absolute timestamps so the `fps` filter's output grid stays aligned with the
    source frames (with reset timestamps it duplicates the first frame);
  - `trim=start=<start_time + target>` makes the cut exact on those absolute timestamps;
  - containers with a seek index (MP4/MOV, Matroska/WebM, AVI) seek directly to the target; others
    (e.g. MPEG-TS, where ffmpeg may land after the target) seek 5 s earlier and let `trim` cut.
- **Encode**: processed `rgb24` frames written to the stdin of
  `ffmpeg -f rawvideo -pix_fmt rgb24 -s WxH -framerate FPS -i pipe:0 [-ss S -t D -i IN -map 0:v:0 -map 1:a:0?]
  -vf "[crop to even,]scale=out_color_matrix=bt709:out_range=tv,format=PIXFMT" <codec args> [audio args] [extra args] OUT`.
  YUV outputs are tagged BT.709; odd sizes are cropped by one pixel for chroma-subsampled formats.
  The audio input is limited with `-t` (not `-shortest`, which drops trailing video frames).
- stderr of every ffmpeg process is drained on a thread; its last lines become the error message.
  Progress is counted on our side (frames written). Processes are killed when dropped and spawned
  without a console window on Windows.
- Distribution: release builds bundle a GPL ffmpeg build on Windows and macOS (Tauri sidecar); Linux
  packages depend on the distribution's ffmpeg (the AppImage bundles one).

### 3.4 HTTP / WebSocket API (v1)

All JSON unless noted; errors are `{"error": "…"}` with 400 (invalid input), 403 (path outside the
roots), 409 (no video / render running / output exists), 422 (not a video), 501 (no dialogs).
Paths in requests are checked against the roots in server mode (relative = relative to the first
root).

| Method & path | Purpose |
|---|---|
| `GET /` (and any non-API path) | Frontend SPA (embedded `web/dist`) |
| `GET /api/info` | `{version, mode, ffmpeg: {ok, version, error}, roots, dialogs}` |
| `GET /api/session` | `{video, preset, rev, cache, render, settings}` — everything a client needs on connect |
| `POST /api/open` `{path}` | Open a video → `{info, preview_width, preview_height}` |
| `GET /api/preset`, `PUT` (replace), `PATCH` (JSON merge patch) | Current preset → `{preset, rev}` |
| `POST /api/preset/save`, `/api/preset/load` `{path}` | Preset files |
| `GET/PUT /api/settings` | Persisted settings |
| `GET /api/render` | Current/last job (`JobStatus` or null) |
| `POST /api/render` `{output, overwrite}` | Start a render job with the current preset |
| `DELETE /api/render` | Cancel → `{cancelled}` |
| `POST /api/render/command` `{output}` | ffmpeg command the render would run → `{command}` |
| `GET /api/frame/{i}?view=output\|original&full=false` | Frame as PNG (preview cache, or decoded at full resolution) |
| `GET /api/audio` | The video's audio as WAV (16-bit stereo 48 kHz, HTTP range requests supported); 404 without audio |
| `GET /api/fs/list?path=` | Directory listing `{path, parent, entries: [{name, path, dir, size}], available}` (hidden files skipped; `available` = bytes free for this user on its filesystem, null if unknown) |
| `POST /api/fs/upload?dir=&name=[&overwrite=true][&unique=true]` | Raw request body (no size limit) stored as `dir/name` via a hidden `.name.part` file renamed on completion → `{path, size}`; with `unique` a taken name becomes `name (2).ext` etc. (the UI always sends it); otherwise 409 if it exists; 400 for names with path components or a leading dot |
| `GET /api/fs/download?path=` | File as an attachment (`Content-Disposition` with UTF-8 name, range requests supported); 400 for directories |
| `POST /api/dialog/{open-video\|save-video\|open-preset\|save-preset}` | Native dialog (desktop) → `{path}` |
| `WS /api/ws` | Frame requests/responses and server events |

`JobStatus`: `{id, state: running|finished|failed|cancelled, done, total, fps, eta_seconds,
elapsed_seconds, output, command, error}`. `rev` increases whenever comparison or effect
parameters change (render options do not change it).

**WebSocket protocol.** Text messages are JSON, binary messages are frames.

- Client → server: `{"type":"view","seq":n,"frame":i,"view":"output|original"}`.
- Server → client binary frame: 32-byte header of eight little-endian `u32`:
  `magic "MXF1", seq, frame index, width, height, format (0 = RGB8 raw, 1 = JPEG),
  flags (1 = exact, 2 = original view), rev`, followed by the payload.
- **Latest wins**, per view: each view (output/original) has one slot per client; a request
  replaces the pending one and aborts the one being waited on. If the frame is not cached within
  15 ms, a stand-in (nearest cached frame, flag *exact* unset) is sent first, then the exact frame.
- Raw RGB on loopback connections; JPEG for remote clients. Lossy encoding is only ever applied
  *after* the effect, never to source frames (compression artifacts would show up as false motion).
- Server events (text): `{"type":"opened","video":…}`, `{"type":"video","video":…}` (the video
  has fewer frames than probed; clients keep their playhead), `{"type":"preset","preset":…,"rev":n}`,
  `{"type":"cache","cache":{ranges,capacity,cached,decoding}}` (≤ 5/s), `{"type":"render","job":…}`
  (≤ 4/s plus final state), `{"type":"settings","settings":…}`, `{"type":"error","message":…}`.

### 3.5 Frontend (`web/`)

- Svelte 5 (runes) + TypeScript (strict), built with Vite into `web/dist`, which `mx-server` embeds
  (read from disk in debug builds). No UI framework beyond Svelte; small hand-written components.
- `src/lib/`: `types.ts` (mirrors of the backend JSON), `api.ts` (REST client), `socket.ts`
  (WebSocket with reconnect and per-view sequence numbers), `protocol.ts` (binary frame header),
  `store.svelte.ts` (the single reactive app store: session state, playhead, playback loop, view
  mode, coalesced preset patches, toasts), `renderer.ts` (WebGL2 drawing with Canvas 2D fallback),
  helpers `time.ts`, `paths.ts` (POSIX + Windows), `args.ts`, `merge.ts`, `size.ts`.
- `src/components/`: `TopBar`, `Viewer` (view modes, zoom/pan, split handle), `Timeline` (scrub
  track with cached ranges, reference marker, render range), `ParamsPanel`, `RenderDialog`,
  `SettingsDialog`, `FileBrowser` (server mode; native dialogs in desktop mode), `Modal`,
  `Slider`, `Toasts`, `Icon` (inline SVG — no dependence on system fonts/emoji).
- Parameter edits apply locally at once and are sent as JSON merge patches; while one request is
  in flight further edits are coalesced into the next. `preset` events from the server are adopted
  only if they differ from what this client last sent (other clients, preset loads).
- Playback (`play`, `tick`, `startClock`, `clockFrame` in the store) uses the pure helpers in
  `src/lib/playback.ts` (`isCached`, `bufferEnd`, `lagging`; unit-tested). An `Audio` element per
  opened video loads `/api/audio?v=<n>`; if it cannot play, playback continues silently.
- Render job updates from the start request and from events are applied in order: an update older
  than the shown state (same job id, terminal → running) is ignored.
- Frames: the store requests the views the current mode needs (`output`, `original` or both) on
  every playhead/mode/revision change; responses older than the newest shown one are dropped.
  Playback advances the playhead by wall-clock time, so slow frames are skipped, not slowed.
- The viewer's "Loading…" badge appears only when a frame is pending and no new frame has arrived
  for 300 ms, so continuous playback and scrubbing (where the playhead is always slightly ahead of
  the frames) do not make it flicker.
- Viewer uploads RGB8 frames as WebGL textures (`texImage2D`), JPEG frames via `createImageBitmap`;
  nearest-neighbour filtering from 200% zoom. 100% = one preview pixel per CSS pixel.
- Keyboard: Space play/pause, ←/→ ±1 frame, Shift+←/→ ±1 s, Home/End (trim bounds), I/O (trim in/out
  at the playhead) — ignored while typing.
- Trim state lives in the store as `trim` (`[start, end)` from `preset.render`, whole video when
  unset) with `setTrim`, `goStart`, `goEnd`; a trim covering the whole video is stored as `null`s.
- Slider reset buttons compare against `defaultPreset()` in `types.ts`, which must match the backend
  defaults (an E2E test checks that a fresh session shows no reset button).
- Testability: every control has a stable `data-testid`; the viewer exposes `data-frame`,
  `data-exact`, `data-rev`, `data-mode`; `window.mxDebug.store.sample(view, x, y)` returns the RGB
  value of the last received raw frame.
- Dark theme with CSS custom properties (`src/app.css`).

---

## 4. Preview engine

The engine makes scrubbing instant by separating **slow** work (decoding) from **fast** work (the
effect at preview resolution).

### 4.1 Frame cache (`engine::cache`)

- Holds decoded **source** frames at preview resolution (`rgb24`), keyed by frame index, shared as
  `Arc<Frame>`. Cost per frame = `w × h × 3` bytes (960×540 ≈ 1.5 MB → 2 GiB ≈ 1400 frames ≈ 47 s
  at 30 fps). Capacity = budget / frame size (at least 3 frames).
- If the whole video fits, the cache fills completely in the background and everything becomes
  random access.
- **Eviction** is distance based: the frame furthest from the playhead goes first, where frames
  behind the playhead count double. Pinned frames (current, its reference, the fixed reference
  frame) are never evicted. A frame that would itself be the first victim is rejected.
- Opening a video or changing the preview size resets the cache (a generation counter makes stale
  inserts from old decoders no-ops).

### 4.2 Prefetch scheduler (`engine::prefetch`)

- The **window** is the set of frames kept around the playhead: `capacity − 1` frames (one slot
  reserved for the reference), one third behind and two thirds ahead (matching the eviction
  metric), shifted inwards at the ends of the video; the whole video if it fits.
- Decoding happens in **runs**: a worker (one ffmpeg process) starts at a missing frame and streams
  forward until the run reaches a cached frame, leaves the window, is rejected by the cache, or is
  preempted. Runs never extend outside the window.
- Run start priority: current frame, its reference, forward from the playhead to the window end,
  then backward to the window start. Frames within 48 frames ahead of another worker's position are
  left to that worker.
- On a focus change, runs outside the new window are preempted; if the needed frames are not being
  decoded and every worker is busy, the run furthest from the playhead is preempted.
- Frames past a clean end of stream (the video is shorter than probed) are remembered and skipped.
  Decoder errors (e.g. ffmpeg failed to start) are retried after 250 ms; a frame is given up after
  three failed attempts. After a rejected insert the workers stall until the focus changes.
- Preview decoders run ffmpeg with `-threads 1` for the raw output stage (decoding itself stays
  multi-threaded) to keep the thread count per decoder low.

### 4.3 Processing (`engine::session`)

- `Session::view(t, view, wait, abort)` points cache and prefetcher at `t`, waits for the source
  frames, and computes the output with `core::process_prepared` at preview resolution.
- LRUs: processed images keyed by `(frame, view, rev)` (24 entries) and denoised `Prepared` frames
  keyed by `(frame, denoise)` (8 entries).
- `approximate_view` processes the nearest cached frame as a stand-in.
- `full_frame` decodes the frame (and its reference) at native resolution, bypassing the cache.
- `audio_file` extracts the audio once per opened video into a temporary WAV
  (`<temp>/motion-extractor-<pid>-<generation>.wav`, via `Ffmpeg::extract_audio_wav`), removed when
  another video is opened or the session is dropped. WAV because every webview can decode it
  (WebKitGTK without extra GStreamer plugins).
- When a decoder run hits the end of the stream before the probed frame count, the prefetcher
  reports `PrefetchEvent::StreamEnded(n)`; the session lowers the frame count (and duration) and
  sends a `video` event, so playback and renders do not wait for frames that do not exist.

### 4.4 Render path

Rendering does not use the preview cache: it runs its own decoder thread(s) → `core::process_prepared`
(rayon, main thread) → encoder thread pipeline with bounded channels (4 frames) between stages.

- Rolling mode keeps the frames still needed (`|k| + 1`) in a ring buffer if that fits in 512 MiB;
  otherwise a second decoder streams the reference frames in lockstep. Negative offsets read ahead.
- Fixed mode decodes the reference frame once.
- `--height` scales in the decoder, so processing runs at output size.
- On error or cancellation the encoder is killed and the partial output file deleted.
- Reference throughput (1080p, 16 threads, FFV1 output): ~100 fps plain, ~55 fps with bloom,
  ~40 fps with denoise.

---

## 5. Testing

**Rule: every user-facing feature in §2 has at least one E2E test. A feature is not done until its
tests are.** Tests never rely on binary fixtures in the repo; test videos are generated with ffmpeg
`lavfi` sources at test setup time (e.g. a white square moving at a known speed over black, encoded
losslessly so pixel values are exact).

| Level | Tooling | Scope |
|---|---|---|
| Unit (Rust) | `cargo test`, `proptest` | `core` effect math with synthetic frames (e.g. identical frames → black, known diff → known mask, resolution independence of blur/bloom), cache eviction policy, prefetch window/priority, settings, ffmpeg argument building, WS header encoding, auth parsing, JSON merge patch |
| Session (Rust) | `crates/engine/tests/session.rs` | open, views, parameter changes, approximations, settings reopen, render jobs incl. cancel, prefetch window behaviour |
| API (Rust) | `crates/server/tests/api.rs` (reqwest, tokio-tungstenite) | every endpoint, path restrictions, token/Host/Origin checks, WebSocket latest-wins frames and events |
| Integration (Rust) | `cargo test` with real ffmpeg | probe, decode, frame-accurate seeking (MP4, MKV, MPEG-TS; 25/29.97/30 fps), every codec, render pipeline (`crates/engine/tests`): render fixture → decode output → assert motion pixels where the square moved and black elsewhere; ring vs. dual decoder equivalence; cancellation |
| CLI (Rust) | `crates/app/tests/cli.rs` runs the built binary | every `render` option, presets, overwrite protection, error reporting |
| Unit (web) | Vitest | stores, unit conversion (frames ↔ seconds), WS protocol parsing |
| E2E | Playwright (Chromium + WebKit) against `motion-extractor serve` (`e2e/`) | see the coverage table below |
| Desktop smoke | CI on Linux (Xvfb), Windows and macOS | binary launches the window, UI loads, then exits (`--smoke-test`) |

**E2E harness** (`e2e/tests/fixtures.ts`): every test gets its own server process
(`serve --port 0 --root <tmp> --settings <tmp>/settings.json`, URL parsed from stdout) and root
directory (teardown waits for the server to exit and retries removing the directory — on Windows
files stay locked until the server's ffmpeg processes are gone); fixture videos are generated per worker with ffmpeg (same moving square as
`mx_media::testing`, FFV1 + FLAC, exact pixels). Pixel assertions read the frames the UI actually
received via `window.mxDebug.store.sample`; frame/revision synchronisation uses the viewer's
`data-frame`/`data-exact`/`data-rev` attributes. `MX_BIN` overrides the binary
(default `target/debug/motion-extractor`), `MX_BROWSERS=chromium,webkit` selects engines
(Chromium ≈ WebView2, WebKit ≈ WebKitGTK/WKWebView).

| Feature (§2) | E2E spec |
|---|---|
| Open via file browser, navigation, non-video errors, connection state | `open.spec.ts` |
| Timeline scrubbing, frame stepping, keyboard shortcuts, playback, cache fill, no loading-badge flicker during playback, buffering when decoding is slower than playback (ffmpeg throttled by `tests/support/slow-ffmpeg.mjs`) | `playback.spec.ts` |
| Audio playback along with the video, mute and volume 0–200 % (remembered), no audio controls without audio | `audio.spec.ts` |
| Rolling offset (frames/seconds, unit switch, negative, zero rejected), fixed reference | `comparison.spec.ts` |
| Threshold/gain/gamma, color picker, bloom, solid background color, motion showing the original, overlay (brightness, opacity), classic mode, metric, denoise, slider reset buttons | `effect.spec.ts` |
| Original/split/side-by-side views, split handle, zoom/pan/fit | `views.spec.ts` |
| Trim: dragging the edges, playback/jumps within the trim, I/O keys, reset, render of the trimmed part, cleared on open | `trim.spec.ts` |
| Render dialog: defaults, command preview, codecs, CRF, speed, pixel format, extra args, audio, range, resolution, PNG sequence, output browser, overwrite confirmation, cancel, errors, progress chip | `render.spec.ts` |
| Settings: memory estimate, persistence, preview resolution, validation | `settings.spec.ts` |
| Preset save/load, invalid presets | `presets.spec.ts` |
| Upload (opens the video, no overwrite; several files at once, no type filter), free space, download from the file browser and of finished renders | `files.spec.ts` |
| Parameter sync between clients | `sync.spec.ts` |

---

## 6. Build, run, test

Tooling and tasks are defined in `mise.toml` ([mise](https://mise.jdx.dev)): `mise install` installs
the pinned Node.js, Rust (stable, with rustfmt/clippy) and actionlint; `mise tasks` lists the tasks.
Not managed by mise (system prerequisites): ffmpeg + ffprobe (with libx264, libx265, libvpx) on
`PATH`, and the Tauri system libraries for the desktop app. The plain `cargo`/`npm` commands behind
each task keep working without mise. CI runs the same tasks.

```
mise run build            # release build (desktop + CLI) → target/release/motion-extractor
mise run build:headless   # release build without the desktop window (serve/render/probe)
mise run desktop [-- FILE]   # run the desktop app
mise run serve [-- ARGS]     # web UI without a window at http://127.0.0.1:8090/
mise run dev              # Vite dev server with hot reload (proxies /api to a running `serve`)
mise run bundle           # installers for this platform (Tauri)
mise run test             # Rust (needs ffmpeg) + web unit/integration tests
mise run e2e              # Playwright E2E (MX_BROWSERS=chromium,webkit for both engines)
mise run lint             # rustfmt, clippy (± desktop), svelte-check, ESLint, Prettier, tsc, actionlint
mise run fmt              # format everything
mise run smoke            # desktop smoke test (needs a display)
mise run ci               # lint + test + e2e
```

Task dependencies build the web UI (`web:build`, embedded by `crates/server`) and install npm
dependencies (`web:install`, `e2e:install`) and Playwright browsers (`e2e:browsers`) as needed.

**Deploying to a server:** `scripts/quick-deploy.sh HOST` (manual use, not run by CI) cross-compiles
the headless binary for a Linux host (architecture and glibc version detected over SSH; `cargo
zigbuild`, with zig and cargo-zigbuild provided by mise), uploads it to
`~/.local/bin/motion-extractor` and (re)starts a systemd **user** service running
`serve --bind 0.0.0.0` (port `MX_PORT`, default 8090; root `MX_ROOT`, default `~/motion-extractor`).
The unit is only created if missing; lingering is enabled so the service keeps running after
logout. The host needs ffmpeg; the script warns if firewalld has the port closed.

## 7. CI / CD (GitHub Actions)

- **`.github/workflows/ci.yml`** (push to `main`, pull requests, manual), tools via `jdx/mise-action`:
  - *Lint* (Ubuntu): `mise run lint` and `mise run test:web`.
  - *Test* matrix — Ubuntu 24.04 (Chromium + WebKit), Windows (Chromium), macOS (Chromium + WebKit):
    install ffmpeg (apt / choco / brew), `mise run test:rust`, browsers with system dependencies,
    `mise run e2e`, desktop smoke test (`--smoke-test`, under `xvfb-run` on Linux). Reports and traces
    are uploaded on failure.
- **`.github/workflows/build.yml`** (reusable, input `label`): the platform build matrix, on native
  runners for each architecture —
  Linux x86_64 and arm64 (Ubuntu 22.04 / 22.04-arm for glibc compatibility): `.deb` and `.rpm`
  (system ffmpeg) and an `.AppImage` with bundled ffmpeg; Windows x86_64: `.msi` and NSIS
  `-setup.exe` with bundled ffmpeg; Windows arm64 (`windows-11-arm`): NSIS `-setup.exe` only (WiX
  cannot build ARM64 MSIs); both Windows targets also get a portable zip
  (`motion-extractor-<label>-<target>-portable.zip`: a folder with the desktop exe, `ffmpeg.exe` and
  `ffprobe.exe`; needs the WebView2 runtime, which ships with Windows 10/11); macOS: universal (arm64 + x86_64) `.dmg` with bundled (lipo'd) ffmpeg;
  plus headless CLI archives
  (`motion-extractor-cli-<label>-<target>`, built `--no-default-features`). File names carry the
  label instead of the crate version. Artifacts `dist-<target>` are kept for 7 days.
  - `scripts/fetch-ffmpeg.sh <target> crates/app/binaries` downloads static GPL builds (BtbN for
    Linux/Windows x86_64 and arm64, martin-riedl.de for macOS with SHA-256 check) named `ffmpeg-<target>` for
    `tauri.ffmpeg.conf.json`.
- **`.github/workflows/release.yml`** (tag `vX.Y`, or manual for a dry run): a version check, then `build.yml` with `X.Y` as
  label, then a **draft** GitHub release with all files (published by hand).
- **`.github/workflows/nightly.yml`** (daily 02:17 UTC, or manual): if `main` moved since the last
  nightly (the `nightly` tag), `build.yml` with label `nightly-YYYYMMDD-<sha>`, then the rolling
  `nightly` pre-release (not marked latest) is deleted and recreated with the new files — storage
  stays at one set of builds.
- Builds are not code-signed or notarised yet (macOS: right-click → Open; Windows: SmartScreen).
- **Disclaimer**: `.github/RELEASE_DISCLAIMER.md` (the project is vibe coded; the maintainer only
  uses the Linux CLI binary in `serve` mode; Windows/macOS builds and .deb/.rpm/.AppImage packages are built
  automatically — Windows and macOS also run the test suite — but not used by the maintainer:
  provided for convenience, at your own risk) is shown at the top of the README and
  prepended to the notes of every release and of the nightly pre-release. Keep the three in sync by
  editing only that file (and the copy in the README).
- Workflows are checked with `actionlint` (part of `mise run lint`).

**Versioning:** versions have two numbers, `X.Y` (tags and releases `vX.Y`). `X` is increased for
breaking changes of the HTTP/WebSocket API or the CLI, otherwise `Y`. Cargo and npm require three
numbers, so the manifests say `X.Y.0` (the patch number is always 0); `motion-extractor --version`
and `GET /api/info` report `X.Y`.

**Releasing:** `scripts/set-version.sh X.Y` (sets the version in `Cargo.toml`, `Cargo.lock` and the
`web`/`e2e` packages), merge that through a PR, tag `vX.Y` on `main`, push the tag, review and
publish the draft release. `release.yml` rejects tags that are not `vX.Y` or do not match the
workspace version; file names carry `X.Y`.

## 8. Conventions

- Rust edition 2024, `cargo fmt`, clippy clean with `-D warnings`. Errors: `thiserror` in libraries,
  `anyhow` in the binary. Logging via `tracing`.
- TypeScript strict mode, ESLint + Prettier.
- `core` stays pure (no IO, no async, no global state) so it is trivially testable.
- No platform-specific code outside `app` (desktop shell) and `media` (process spawning details).
- All changes go through **pull requests**; `main` is only changed by merging a PR, and a PR is
  only merged when its CI checks (Lint, Test on Linux, Windows and macOS) are green. Once the
  repository is public, a ruleset on `main` enforces this (required PR + required status checks).
- Update this file together with any change to behaviour or structure.

## 9. Roadmap

- [x] M1 — `core` effect pipeline + `media` + `render`/`probe` CLI, with unit/integration tests
  (incl. classic mode and presets)
- [x] M2 — `engine` (cache, prefetch, processing, settings, render jobs) + `server` API + WebSocket
  frames + `serve` command
- [x] M3 — web frontend: viewer, timeline, parameter panel, render & settings dialogs, file browser
- [x] M4 — Playwright E2E suite covering §2 (Chromium and WebKit)
- [x] M5 — desktop shell (Tauri), native dialogs, drag & drop, smoke test, installers (.deb/.rpm verified)
- [x] M6 — GitHub Actions CI + release pipeline with installers, rolling nightly builds, mise tasks
  (workflows not yet run on GitHub)
- [x] Trim on the timeline, slider reset buttons
- [x] Audio playback, buffering instead of freezing, corrected frame counts for short streams
- [ ] Later — alpha output, Flatpak, presets library, faster bloom compositing

## 10. Licensing

The project is licensed under **GPL-3.0-or-later** (see `LICENSE`). This allows bundling GPL ffmpeg
builds (with x264/x265) in release packages.

## 11. Open questions

- **Name** — "Motion Extractor" / `motion-extractor` is a working title.
- **Repository URL** — not set in `Cargo.toml` yet (add `repository` once the GitHub repo exists).
- **Code signing** for Windows/macOS releases.
