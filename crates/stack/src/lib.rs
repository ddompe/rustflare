//! Lucky-imaging stacker for RustFlare recordings. See `docs/project/proposal-onboard-stacking.md`.

pub mod align;
pub mod calib;
pub mod fft2;
pub mod fits;
pub mod image;
pub mod multipoint;
pub mod output;
pub mod ser;
pub mod session;
pub mod simd;
pub mod source;
pub mod stack;
pub mod telemetry;
pub mod watch;
