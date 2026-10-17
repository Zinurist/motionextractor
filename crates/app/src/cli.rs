use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use mx_core::{Background, BlendMode, Comparison, Metric, MotionFill, Offset, Rgb};
use mx_engine::{Preset, Progress, RenderSpec};
use mx_media::{Audio, Codec, Ffmpeg, command_line};

use crate::timespec::TimeSpec;

#[derive(Debug, Parser)]
#[command(
    name = "motion-extractor",
    version,
    about = "Highlight the motion in videos"
)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    /// Video to open in the desktop app (when no subcommand is given).
    file: Option<PathBuf>,
    /// Open the window, wait for the UI to load, then exit (for CI).
    #[arg(long, hide = true)]
    smoke_test: bool,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print information about a video as JSON.
    Probe { input: PathBuf },
    /// Render the motion extraction of a video to a file.
    Render(Box<RenderArgs>),
    /// Serve the web UI without a window (open the printed URL in a browser).
    Serve(crate::serve::ServeArgs),
}

#[derive(Debug, Args)]
struct RenderArgs {
    input: PathBuf,
    /// Output file (for PNG sequences: a directory or a pattern like `frames/%06d.png`).
    output: PathBuf,

    /// Load effect and render settings from a preset file; other options override it.
    #[arg(long, value_name = "FILE")]
    preset: Option<PathBuf>,
    /// Save the effective settings as a preset file.
    #[arg(long, value_name = "FILE")]
    save_preset: Option<PathBuf>,
    /// Overwrite the output if it exists.
    #[arg(short = 'y', long)]
    overwrite: bool,
    /// Print the ffmpeg encoder command and exit without rendering.
    #[arg(long)]
    dry_run: bool,
    /// No progress output.
    #[arg(short, long)]
    quiet: bool,

    #[command(flatten, next_help_heading = "Comparison")]
    comparison: ComparisonArgs,
    #[command(flatten, next_help_heading = "Effect")]
    effect: EffectArgs,
    #[command(flatten, next_help_heading = "Output")]
    output_opts: OutputArgs,
}

#[derive(Debug, Args)]
struct ComparisonArgs {
    /// Distance to the compared frame: frames (`3`, `3f`) or seconds (`0.5s`). Negative values
    /// compare with a later frame. Default: 1 frame.
    #[arg(long, allow_hyphen_values = true, conflicts_with = "reference")]
    offset: Option<Offset>,
    /// Compare every frame with this fixed frame instead (`120`, `2.5s`, `1:02.5`).
    #[arg(long, value_name = "TIME")]
    reference: Option<TimeSpec>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum MetricArg {
    Luma,
    Max,
}

#[derive(Debug, Args)]
struct EffectArgs {
    /// How channel differences are combined.
    #[arg(long)]
    metric: Option<MetricArg>,
    /// Differences below this (0–1) are ignored. Default 0.02.
    #[arg(long)]
    threshold: Option<f32>,
    /// Amplification of differences. Default 4.
    #[arg(long)]
    gain: Option<f32>,
    #[arg(long)]
    gamma: Option<f32>,
    /// Pre-blur radius as a fraction of the frame height (e.g. 0.002) to suppress noise.
    #[arg(long)]
    denoise: Option<f32>,
    /// Motion color as #rrggbb. Default white.
    #[arg(long)]
    color: Option<Rgb>,
    /// Add a glow around motion.
    #[arg(long)]
    bloom: bool,
    /// Bloom radius as a fraction of the frame height. Default 0.02.
    #[arg(long)]
    bloom_radius: Option<f32>,
    #[arg(long)]
    bloom_intensity: Option<f32>,
    #[arg(long)]
    bloom_threshold: Option<f32>,
    /// What moving parts show: the motion color, or the original video (seen through the motion).
    #[arg(long)]
    motion_fill: Option<FillArg>,
    /// Draw the motion on top of the original video instead of a solid color.
    #[arg(long)]
    overlay: bool,
    /// Solid background color as #rrggbb (when not using --overlay). Default black.
    #[arg(long)]
    background_color: Option<Rgb>,
    /// Brightness of the original video in overlay mode (0–1).
    #[arg(long)]
    background_brightness: Option<f32>,
    /// Opacity of the motion in overlay mode (0–1).
    #[arg(long)]
    overlay_opacity: Option<f32>,
    /// Classic mode: average of the frame and the inverted reference (grey image).
    #[arg(long)]
    classic: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum FillArg {
    Color,
    Original,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CodecArg {
    H264,
    H265,
    Vp9,
    Prores,
    Ffv1,
    Png,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum AudioArg {
    Encode,
    Copy,
    None,
}

#[derive(Debug, Args)]
struct OutputArgs {
    /// Video codec. Default: derived from the output extension (mp4 → h264, webm → vp9, …).
    #[arg(long)]
    codec: Option<CodecArg>,
    /// Quality (lower is better) for h264/h265/vp9.
    #[arg(long)]
    crf: Option<u32>,
    /// Encoder speed preset (x264/x265: ultrafast…veryslow; vp9: cpu-used 0–8; prores: profile 0–5).
    #[arg(long)]
    encoder_preset: Option<String>,
    #[arg(long)]
    pix_fmt: Option<String>,
    /// Output height in pixels (aspect ratio preserved).
    #[arg(long)]
    height: Option<u32>,
    /// First frame to render (`120`, `2.5s`, `1:02.5`).
    #[arg(long, value_name = "TIME")]
    start: Option<TimeSpec>,
    /// End of the rendered range (exclusive).
    #[arg(long, value_name = "TIME")]
    end: Option<TimeSpec>,
    #[arg(long)]
    audio: Option<AudioArg>,
    /// Extra ffmpeg output argument (repeatable), e.g. `--ffmpeg-arg=-tune --ffmpeg-arg=grain`.
    #[arg(long = "ffmpeg-arg", value_name = "ARG", allow_hyphen_values = true)]
    ffmpeg_args: Vec<String>,
}

impl Cli {
    /// Whether a CLI subcommand was given (as opposed to starting the desktop app).
    pub fn is_command(&self) -> bool {
        self.command.is_some()
    }

    pub fn run(self) -> Result<()> {
        let command = match self.command {
            None => return desktop(self.file, self.smoke_test),
            Some(Command::Serve(args)) => return crate::serve::run(args),
            Some(c) => c,
        };
        let ff = Ffmpeg::locate()?;
        match command {
            Command::Serve(_) => unreachable!("handled above"),
            Command::Probe { input } => {
                let info = ff.probe(&input)?;
                println!("{}", serde_json::to_string_pretty(&info)?);
                Ok(())
            }
            Command::Render(args) => render(&ff, *args),
        }
    }
}

#[cfg(feature = "desktop")]
fn desktop(file: Option<PathBuf>, smoke_test: bool) -> Result<()> {
    crate::desktop::run(file, smoke_test)
}

#[cfg(not(feature = "desktop"))]
fn desktop(_file: Option<PathBuf>, _smoke_test: bool) -> Result<()> {
    bail!("this build has no desktop window; use `motion-extractor serve` (see --help)")
}

fn render(ff: &Ffmpeg, args: RenderArgs) -> Result<()> {
    let info = ff.probe(&args.input)?;
    let fps = info.fps_f64();
    let mut preset = match &args.preset {
        Some(path) => Preset::load(path)?,
        None => {
            let mut p = Preset::default();
            p.render.encode.codec = Codec::from_path(&args.output);
            p
        }
    };
    apply_overrides(&mut preset, &args, fps);
    preset.comparison.validate()?;
    preset.effect.validate()?;

    if let Some(path) = &args.save_preset {
        preset.save(path)?;
    }

    let spec = RenderSpec {
        output: args.output.clone(),
        comparison: preset.comparison,
        effect: preset.effect,
        options: preset.render,
    };
    let plan = mx_engine::plan(ff, &info, &spec)?;
    if args.dry_run {
        println!("{}", command_line(&ff.ffmpeg, &plan.encoder_args));
        return Ok(());
    }
    check_output(
        &plan.target.output_path(spec.options.encode.codec),
        args.overwrite,
    )?;

    let mut reporter = ProgressReporter::new(!args.quiet && std::io::stderr().is_terminal());
    let started = Instant::now();
    let plan = mx_engine::render(ff, &info, &spec, &AtomicBool::new(false), |p| {
        reporter.update(p)
    })
    .context("render failed")?;
    reporter.finish();
    if !args.quiet {
        eprintln!(
            "rendered {} frames ({}x{}) to {} in {:.1}s",
            plan.frame_count(),
            plan.width,
            plan.height,
            args.output.display(),
            started.elapsed().as_secs_f64()
        );
    }
    Ok(())
}

fn apply_overrides(p: &mut Preset, args: &RenderArgs, fps: f64) {
    let c = &args.comparison;
    if let Some(offset) = c.offset {
        p.comparison = Comparison::Rolling { offset };
    }
    if let Some(r) = c.reference {
        p.comparison = Comparison::Fixed {
            frame: r.to_frame(fps),
        };
    }

    let e = &args.effect;
    let fx = &mut p.effect;
    if let Some(m) = e.metric {
        fx.metric = match m {
            MetricArg::Luma => Metric::Luma,
            MetricArg::Max => Metric::Max,
        };
    }
    set(&mut fx.threshold, e.threshold);
    set(&mut fx.gain, e.gain);
    set(&mut fx.gamma, e.gamma);
    set(&mut fx.denoise, e.denoise);
    set(&mut fx.color, e.color);
    if e.bloom {
        fx.bloom.enabled = true;
    }
    set(&mut fx.bloom.radius, e.bloom_radius);
    set(&mut fx.bloom.intensity, e.bloom_intensity);
    set(&mut fx.bloom.threshold, e.bloom_threshold);
    if let Some(f) = e.motion_fill {
        fx.motion_fill = match f {
            FillArg::Color => MotionFill::Color,
            FillArg::Original => MotionFill::Original,
        };
    }
    if e.overlay {
        fx.background = Background::Original;
    }
    set(&mut fx.background_color, e.background_color);
    set(&mut fx.background_brightness, e.background_brightness);
    set(&mut fx.overlay_opacity, e.overlay_opacity);
    if e.classic {
        fx.blend = BlendMode::Classic;
    }

    let o = &args.output_opts;
    let r = &mut p.render;
    if let Some(codec) = o.codec {
        r.encode.codec = match codec {
            CodecArg::H264 => Codec::H264,
            CodecArg::H265 => Codec::H265,
            CodecArg::Vp9 => Codec::Vp9,
            CodecArg::Prores => Codec::Prores,
            CodecArg::Ffv1 => Codec::Ffv1,
            CodecArg::Png => Codec::Png,
        };
    }
    if o.crf.is_some() {
        r.encode.crf = o.crf;
    }
    if o.encoder_preset.is_some() {
        r.encode.encoder_preset.clone_from(&o.encoder_preset);
    }
    if o.pix_fmt.is_some() {
        r.encode.pix_fmt.clone_from(&o.pix_fmt);
    }
    if o.height.is_some() {
        r.height = o.height;
    }
    if let Some(s) = o.start {
        r.start = Some(s.to_frame(fps));
    }
    if let Some(e) = o.end {
        r.end = Some(e.to_frame(fps));
    }
    if let Some(a) = o.audio {
        r.encode.audio = match a {
            AudioArg::Encode => Audio::Encode,
            AudioArg::Copy => Audio::Copy,
            AudioArg::None => Audio::None,
        };
    }
    if !o.ffmpeg_args.is_empty() {
        r.encode.extra_args.clone_from(&o.ffmpeg_args);
    }
}

fn set<T>(target: &mut T, value: Option<T>) {
    if let Some(v) = value {
        *target = v;
    }
}

/// Refuses to overwrite an existing output (or a non-empty PNG directory) without `-y`.
fn check_output(path: &Path, overwrite: bool) -> Result<()> {
    if !overwrite && mx_engine::output_occupied(path) {
        bail!("{} already exists (use -y to overwrite)", path.display());
    }
    Ok(())
}

struct ProgressReporter {
    enabled: bool,
    last: Option<Instant>,
}

impl ProgressReporter {
    fn new(enabled: bool) -> Self {
        Self {
            enabled,
            last: None,
        }
    }

    fn update(&mut self, p: Progress) {
        let due = self
            .last
            .is_none_or(|t| t.elapsed() >= Duration::from_millis(100));
        if !self.enabled || !(due || p.done == p.total) {
            return;
        }
        self.last = Some(Instant::now());
        let eta = p
            .eta()
            .map(|d| format!("{}s", d.as_secs()))
            .unwrap_or_else(|| "?".into());
        let pct = p.done as f64 * 100.0 / p.total as f64;
        eprint!(
            "\r{}/{} frames  {pct:5.1}%  {:.1} fps  ETA {eta}   ",
            p.done,
            p.total,
            p.fps()
        );
        let _ = std::io::stderr().flush();
    }

    fn finish(&self) {
        if self.enabled && self.last.is_some() {
            eprintln!();
        }
    }
}
