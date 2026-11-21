//! Mount drivers. Add a new mount by implementing `rustflare_core::Mount`
//! and adding a case to [`connect`].

pub mod ble;
pub mod simulator;
pub mod zwo;

use rustflare_core::{sim::SimSky, Error, Mount, Result};

/// Names accepted by [`connect`].
pub const DRIVERS: &[&str] = &["zwo", "simulator"];

/// Prefix of a `device` string that selects the Bluetooth transport: `ble:<address>` (stored that way in the
/// startup settings, so old settings files with plain serial paths keep working).
pub const BLE_PREFIX: &str = "ble:";

/// Options that do not belong in the device string.
#[derive(Default, Clone, Debug)]
pub struct ConnectOptions {
    /// Bluetooth session opener override from the settings file (the environment variable wins over it).
    pub ble_token: Option<String>,
}

/// Connects to a mount. `device` is a serial device path, or `ble:<address>` for Bluetooth (ZWO only).
pub fn connect(driver: &str, device: Option<&str>, sky: &SimSky) -> Result<Box<dyn Mount>> {
    connect_with(driver, device, sky, &ConnectOptions::default())
}

pub fn connect_with(driver: &str, device: Option<&str>, sky: &SimSky, opts: &ConnectOptions) -> Result<Box<dyn Mount>> {
    match driver {
        "zwo" => {
            let dev = device.ok_or_else(|| Error::Invalid("ZWO mount needs a serial device path or a Bluetooth address".into()))?;
            if let Some(addr) = dev.strip_prefix(BLE_PREFIX) {
                let env = std::env::var("RUSTFLARE_ZWO_BLE_TOKEN").ok();
                let token = ble::resolve_token(env.as_deref(), opts.ble_token.as_deref())?;
                let (t, alive) = ble::open(addr, &token)?;
                return Ok(Box::new(zwo::Zwo::with_link(t, "Bluetooth", "ble", Some(alive))?));
            }
            Ok(Box::new(zwo::Zwo::open(dev)?))
        }
        "simulator" => Ok(Box::new(simulator::SimMount::new(sky.clone()))),
        other => Err(Error::NotFound(format!("mount driver {other}"))),
    }
}
