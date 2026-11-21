//! Player One cameras through the vendor SDK (`libPlayerOneCamera`), wrapped by
//! the `playerone-sdk` crate. These cameras are *not* V4L2/UVC devices (ADR 0006).
//!
//! NOTE: written against the crate's documented API but not yet exercised on
//! real hardware.

use playerone_sdk::{Camera as PoaCamera, ImageFormat};
use std::time::Duration;
use rustflare_core::camera::*;
use rustflare_core::{Error, Frame, PixelFormat, Result};

pub struct PlayerOneDriver;

fn err<E: std::fmt::Display>(e: E) -> Error { Error::Camera(e.to_string()) }

impl CameraDriver for PlayerOneDriver {
    fn name(&self) -> &'static str { "playerone" }

    fn enumerate(&self) -> Result<Vec<CameraInfo>> {
        Ok(PoaCamera::all_cameras()
            .iter()
            .map(|c| {
                let p = c.properties();
                CameraInfo {
                    driver: "playerone".into(),
                    id: c.camera_id().to_string(),
                    name: format!("{} ({})", p.camera_model_name, p.serial_number),
                }
            })
            .collect())
    }

    fn open(&self, id: &str) -> Result<Box<dyn Camera>> {
        let want: i32 = id.parse().map_err(|_| Error::Invalid(format!("camera id {id}")))?;
        let desc = PoaCamera::all_cameras()
            .into_iter()
            .find(|c| c.camera_id() == want)
            .ok_or_else(|| Error::NotFound(format!("Player One camera {id}")))?;
        let mut cam = desc.open().map_err(err)?;
        let (w, h) = (cam.properties().max_width, cam.properties().max_height);
        cam.set_image_size(w, h).map_err(err)?;
        cam.set_image_format(ImageFormat::RAW16).map_err(err)?;
        Ok(Box::new(PoaCam { cam, format: ImageFormat::RAW16 }))
    }
}

struct PoaCam {
    cam: PoaCamera,
    format: ImageFormat,
}

impl Camera for PoaCam {
    fn info(&self) -> CameraInfo {
        let p = self.cam.properties();
        CameraInfo {
            driver: "playerone".into(),
            id: self.cam.id().to_string(),
            name: format!("{} ({})", p.camera_model_name, p.serial_number),
        }
    }

    fn controls(&mut self) -> Result<Vec<ControlDesc>> {
        let b = self.cam.config_bounds();
        let (exp, _) = self.cam.exposure().map_err(err)?;
        let (gain, _) = self.cam.gain().map_err(err)?;
        let offset = self.cam.offset().map_err(err)?;
        let d = |id: &str, label: &str, min: i64, max: i64, v: i64, unit: Option<&str>, log| ControlDesc {
            id: id.into(), label: label.into(), min: min as f64, max: max as f64, step: 1.0,
            value: v as f64, unit: unit.map(String::from), log,
        };
        Ok(vec![
            d(CTRL_EXPOSURE_US, "Exposure", b.exposure.min, b.exposure.max, exp, Some("µs"), true),
            d(CTRL_GAIN, "Gain", b.gain.min, b.gain.max, gain, None, false),
            d(CTRL_OFFSET, "Offset", b.offset.min, b.offset.max, offset, None, false),
            d(CTRL_HIGH_BIT_DEPTH, "16-bit output", 0, 1, (self.format == ImageFormat::RAW16) as i64, None, false),
        ])
    }

    fn set_control(&mut self, id: &str, value: f64) -> Result<()> {
        let v = value.round() as i64;
        match id {
            CTRL_EXPOSURE_US => self.cam.set_exposure(v, false).map_err(err),
            CTRL_GAIN => self.cam.set_gain(v, false).map_err(err),
            CTRL_OFFSET => self.cam.set_offset(v).map_err(err),
            CTRL_HIGH_BIT_DEPTH => {
                let f = if value >= 0.5 { ImageFormat::RAW16 } else { ImageFormat::RAW8 };
                self.cam.set_image_format(f).map_err(err)?;
                self.format = f;
                Ok(())
            }
            _ => Err(Error::Invalid(format!("unknown control {id}"))),
        }
    }

    fn capture(&mut self, timeout: Duration) -> Result<Frame> {
        let (w, h) = self.cam.image_size();
        let (pf, bpp) = match self.format {
            ImageFormat::RAW16 => (PixelFormat::Mono16, 2),
            _ => (PixelFormat::Mono8, 1),
        };
        let mut buf = self.cam.create_image_buffer();
        self.cam.capture(&mut buf, Some(timeout.as_millis() as i32)).map_err(err)?;
        buf.truncate(w as usize * h as usize * bpp);
        Ok(Frame::new(w, h, pf, buf))
    }

    fn temperature_c(&mut self) -> Option<f64> { self.cam.temperature().ok() }

    fn pixel_size_um(&self) -> Option<f64> {
        let p = self.cam.properties().pixel_size;
        (p > 0.0).then_some(p)
    }
}
