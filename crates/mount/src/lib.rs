//! Mount drivers. Add a new mount by implementing `rustflare_core::Mount`
//! and adding a case to [`connect`].

pub mod simulator;
pub mod zwo;

use rustflare_core::{sim::SimSky, Error, Mount, Result};

/// Names accepted by [`connect`].
pub const DRIVERS: &[&str] = &["zwo", "simulator"];

/// Connects to a mount. `device` is a serial device path for real mounts.
pub fn connect(driver: &str, device: Option<&str>, sky: &SimSky) -> Result<Box<dyn Mount>> {
    match driver {
        "zwo" => {
            let dev = device.ok_or_else(|| Error::Invalid("ZWO mount needs a serial device path".into()))?;
            Ok(Box::new(zwo::Zwo::open(dev)?))
        }
        "simulator" => Ok(Box::new(simulator::SimMount::new(sky.clone()))),
        other => Err(Error::NotFound(format!("mount driver {other}"))),
    }
}
