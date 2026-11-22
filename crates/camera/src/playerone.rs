//! Player One cameras through the vendor SDK (`libPlayerOneCamera`), wrapped by
//! the `playerone-sdk` crate. These cameras are *not* V4L2/UVC devices (ADR 0006).
//!
//! Capture uses the SDK's continuous (video) mode: the exposure is armed once and the camera free-runs,
//! and each `capture` just fetches the next frame. The wrapper's one-shot `capture()` re-arms the camera
//! for every frame, which on the Apollo-M MINI capped it at ~8 fps regardless of exposure time.
//! Changing a setting stops the stream first; the next `capture` restarts it.
//!
//! Verified on real hardware: enumeration, opening, controls (an Apollo-M MINI on a Raspberry Pi 5).

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
        Ok(Box::new(PoaCam { cam, format: ImageFormat::RAW16, streaming: false }))
    }
}

struct PoaCam {
    cam: PoaCamera,
    format: ImageFormat,
    /// True while the camera is in continuous exposure mode.
    streaming: bool,
}

impl PoaCam {
    fn stop_stream(&mut self) {
        if self.streaming {
            let _ = self.cam.stop_exposure();
            self.streaming = false;
        }
    }
}

impl Drop for PoaCam {
    fn drop(&mut self) {
        // The SDK requires stopping a running exposure before closing the camera.
        self.stop_stream();
    }
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

    fn bayer(&self) -> Option<rustflare_core::BayerPattern> {
        use playerone_sdk::BayerPattern as B;
        let p = self.cam.properties();
        if !p.is_color_camera {
            return None;
        }
        match p.bayer_pattern {
            B::RG => Some(rustflare_core::BayerPattern::Rggb),
            B::BG => Some(rustflare_core::BayerPattern::Bggr),
            B::GR => Some(rustflare_core::BayerPattern::Grbg),
            B::GB => Some(rustflare_core::BayerPattern::Gbrg),
            B::MONO => None,
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
        self.stop_stream(); // settings are applied with the exposure stopped; capture() restarts it
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
        if !self.streaming {
            self.cam.start_exposure().map_err(err)?; // continuous mode: arm once, then just fetch frames
            self.streaming = true;
        }
        let mut buf = self.cam.create_image_buffer();
        if let Err(e) = self.cam.get_image_data(&mut buf, Some(timeout.as_millis() as i32)) {
            self.stop_stream(); // re-arm on the next call
            return Err(err(e));
        }
        buf.truncate(w as usize * h as usize * bpp);
        Ok(Frame::new(w, h, pf, buf))
    }

    fn temperature_c(&mut self) -> Option<f64> { self.cam.temperature().ok() }

    fn sensor_size(&self) -> Option<(u32, u32)> {
        let p = self.cam.properties();
        Some((p.max_width, p.max_height))
    }

    fn roi(&self) -> Option<rustflare_core::Roi> {
        let (w, h) = self.cam.image_size();
        let (x, y) = self.cam.image_start_pos().ok()?;
        Some(rustflare_core::Roi { x, y, width: w, height: h })
    }

    fn set_roi(&mut self, roi: rustflare_core::Roi) -> Result<()> {
        let p = self.cam.properties();
        let (mw, mh) = (p.max_width, p.max_height);
        // the SDK wants width a multiple of 4, height of 2, and the window inside the sensor
        let (w, h) = (roi.width / 4 * 4, roi.height / 2 * 2);
        if w == 0 || h == 0 || roi.x + w > mw || roi.y + h > mh {
            return Err(Error::Invalid("region outside the sensor".into()));
        }
        self.stop_stream();
        // growing: the start position first would be out of range for the old size, so go through the origin
        self.cam.set_image_start_pos(0, 0).map_err(err)?;
        self.cam.set_image_size(w, h).map_err(err)?;
        self.cam.set_image_start_pos(roi.x / 2 * 2, roi.y / 2 * 2).map_err(err)
    }

    fn pixel_size_um(&self) -> Option<f64> {
        let p = self.cam.properties().pixel_size;
        (p > 0.0).then_some(p)
    }
}
