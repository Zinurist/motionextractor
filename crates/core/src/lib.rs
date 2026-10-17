//! Pure motion extraction effect pipeline.
//!
//! This crate contains no IO and no global state: frames go in, frames come out. It is shared by the
//! interactive preview and the final render so both produce the same image.

mod blur;
mod color;
mod effect;
mod frame;
mod params;

pub use blur::{gaussian_blur, gaussian_blur_plane};
pub use effect::{EffectError, Prepared, process, process_prepared};
pub use frame::{Frame, FrameError};
pub use params::{
    Background, BlendMode, Bloom, Comparison, EffectParams, Metric, MotionFill, Offset, ParamError,
    Rgb, TimeUnit,
};
