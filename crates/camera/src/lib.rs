//! Camera drivers. Add a new camera by implementing `CameraDriver` / `Camera`
//! from `rustflare-core` and registering it in [`drivers`].

pub mod simulator;
#[cfg(feature = "playerone")]
pub mod playerone;

use rustflare_core::{sim::SimSky, CameraDriver};

/// All drivers compiled into this build.
pub fn drivers(sky: SimSky) -> Vec<Box<dyn CameraDriver>> {
    let mut v: Vec<Box<dyn CameraDriver>> = Vec::new();
    #[cfg(feature = "playerone")]
    v.push(Box::new(playerone::PlayerOneDriver));
    v.push(Box::new(simulator::SimulatorDriver::new(sky)));
    v
}
