//! Hardware-independent types and traits shared by every Rustflare crate.
//!
//! The `Camera` and `Mount` traits are the extension points for new hardware
//! (see ADR 0005). Mount types are adapted from Vidoxide (MIT).

pub mod camera;
pub mod error;
pub mod frame;
pub mod geom;
pub mod mount;
pub mod sim;

pub use camera::{centred_roi, fit_roi, Camera, CameraDriver, CameraInfo, ControlDesc, CoolerRequest, CoolerStatus, Roi, RoiLimits};
pub use error::{Error, Result};
pub use frame::{BayerPattern, ChannelStats, Frame, FrameStats, PixelFormat};
pub use geom::{Mat2, Vec2};
pub use mount::{Axis, LinkInfo, Mount, MountSiteTime, MountState, ParkState, RadPerSec, SiteTime, SlewSpeed, TrackingRate, SIDEREAL_RATE};
