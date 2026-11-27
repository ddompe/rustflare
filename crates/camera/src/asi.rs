//! ZWO ASI cameras through the vendor SDK (`libASICamera2`), loaded at run time (ADR 0014).
//!
//! The library is opened with `dlopen` (via `libloading`), never linked or bundled: the driver builds
//! without the SDK, and a missing library gives a friendly error. On Debian/Raspberry Pi OS the library
//! comes from the `libasi` package. Only the handful of entry points we need are declared here.
//!
//! Capture uses the SDK's video mode: `ASIStartVideoCapture` once, then `ASIGetVideoData` per frame.
//! Changing a setting stops the stream first; the next `capture` restarts it (like the Player One driver).
//!
//! The SDK logic sits behind the small [`Sdk`] trait so the driver can be unit-tested with a fake.

use libloading::Library;
use rustflare_core::camera::*;
use rustflare_core::{Error, Frame, PixelFormat, Result};
use std::ffi::{c_char, c_int, c_long, c_uchar};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

const DRIVER: &str = "asi";
const INSTALL_HINT: &str = "ZWO ASI SDK (libASICamera2) not found; install it with: sudo apt install libasi";

// ASI_CONTROL_TYPE values (ASICamera2.h)
const ASI_GAIN: c_int = 0;
const ASI_EXPOSURE: c_int = 1;
const ASI_OFFSET: c_int = 5;
const ASI_BANDWIDTHOVERLOAD: c_int = 6;
const ASI_TEMPERATURE: c_int = 8;
// ASI_IMG_TYPE
const IMG_RAW8: c_int = 0;
const IMG_RAW16: c_int = 2;
// ASI_ERROR_CODE
const ASI_SUCCESS: c_int = 0;
const ASI_ERROR_TIMEOUT: c_int = 11;

const CTRL_USB_BANDWIDTH: &str = "usb_bandwidth";

#[repr(C)]
#[derive(Clone, Copy)]
struct RawInfo {
    name: [c_char; 64],
    camera_id: c_int,
    max_height: c_long,
    max_width: c_long,
    is_color: c_int,
    bayer: c_int,
    bins: [c_int; 16],
    formats: [c_int; 8],
    pixel_size: f64,
    mechanical_shutter: c_int,
    st4: c_int,
    is_cooler: c_int,
    usb3_host: c_int,
    usb3_camera: c_int,
    elec_per_adu: f32,
    bit_depth: c_int,
    is_trigger: c_int,
    unused: [u8; 16],
    /// Slack in case a newer SDK writes a larger struct than 1.27.
    _slack: [u8; 128],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RawCaps {
    name: [c_char; 64],
    description: [c_char; 128],
    max: c_long,
    min: c_long,
    default: c_long,
    auto_supported: c_int,
    writable: c_int,
    control_type: c_int,
    unused: [u8; 32],
    _slack: [u8; 64],
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Prop {
    pub name: String,
    pub id: i32,
    pub max_w: i32,
    pub max_h: i32,
    pub is_color: bool,
    /// ASI_BAYER_PATTERN: 0 RG, 1 BG, 2 GR, 3 GB.
    pub bayer: i32,
    pub pixel_size_um: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Caps {
    pub control_type: i32,
    pub min: i64,
    pub max: i64,
    pub default: i64,
    pub writable: bool,
}

/// The SDK calls we use. Errors are the raw `ASI_ERROR_CODE`.
pub(crate) trait Sdk: Send + Sync {
    fn num_cameras(&self) -> i32;
    fn property(&self, index: i32) -> std::result::Result<Prop, i32>;
    fn open(&self, id: i32) -> std::result::Result<(), i32>;
    fn init(&self, id: i32) -> std::result::Result<(), i32>;
    fn close(&self, id: i32);
    fn caps(&self, id: i32) -> std::result::Result<Vec<Caps>, i32>;
    fn get_control(&self, id: i32, ctl: i32) -> std::result::Result<i64, i32>;
    fn set_control(&self, id: i32, ctl: i32, v: i64) -> std::result::Result<(), i32>;
    fn set_format(&self, id: i32, w: i32, h: i32, img: i32) -> std::result::Result<(), i32>;
    /// Top-left corner of the readout window; must be set after `set_format`, which centres the window.
    fn set_start_pos(&self, id: i32, x: i32, y: i32) -> std::result::Result<(), i32>;
    fn start_video(&self, id: i32) -> std::result::Result<(), i32>;
    fn stop_video(&self, id: i32);
    fn video_data(&self, id: i32, buf: &mut [u8], wait_ms: i32) -> std::result::Result<(), i32>;
}

fn code_text(c: i32) -> &'static str {
    match c {
        1 => "invalid index (no camera connected)",
        2 => "invalid camera id",
        3 => "invalid control type",
        4 => "camera not open",
        5 => "camera removed (unplugged?)",
        8 => "invalid image size",
        9 => "unsupported image format",
        11 => "timeout waiting for a frame",
        12 => "stop capture first",
        13 => "buffer too small",
        14 => "video mode active",
        15 => "exposure in progress",
        16 => "general error (value out of range?)",
        17 => "invalid mode",
        _ => "unknown error",
    }
}

fn err(what: &str, c: i32) -> Error {
    Error::Camera(format!("ASI {what}: {} (code {c})", code_text(c)))
}

fn cstr(a: &[c_char]) -> String {
    let b: Vec<u8> = a.iter().take_while(|c| **c != 0).map(|c| *c as u8).collect();
    String::from_utf8_lossy(&b).trim().to_string()
}

// ------------------------------------------------------------------ the real library

type FnVoidInt = unsafe extern "C" fn() -> c_int;
type FnInfo = unsafe extern "C" fn(*mut RawInfo, c_int) -> c_int;
type FnId = unsafe extern "C" fn(c_int) -> c_int;
type FnNum = unsafe extern "C" fn(c_int, *mut c_int) -> c_int;
type FnCaps = unsafe extern "C" fn(c_int, c_int, *mut RawCaps) -> c_int;
type FnGet = unsafe extern "C" fn(c_int, c_int, *mut c_long, *mut c_int) -> c_int;
type FnSet = unsafe extern "C" fn(c_int, c_int, c_long, c_int) -> c_int;
type FnRoi = unsafe extern "C" fn(c_int, c_int, c_int, c_int, c_int) -> c_int;
type FnStart = unsafe extern "C" fn(c_int, c_int, c_int) -> c_int;
type FnData = unsafe extern "C" fn(c_int, *mut c_uchar, c_long, c_int) -> c_int;

struct RealSdk {
    // Keeps the shared object mapped for as long as the function pointers below live.
    _lib: Library,
    num: FnVoidInt,
    info: FnInfo,
    open: FnId,
    init: FnId,
    close: FnId,
    num_controls: FnNum,
    caps: FnCaps,
    get: FnGet,
    set: FnSet,
    roi: FnRoi,
    start_pos: FnStart,
    start: FnId,
    stop: FnId,
    data: FnData,
    /// The SDK is not documented as thread-safe; serialise all calls.
    lock: Mutex<()>,
}

impl RealSdk {
    fn load() -> std::result::Result<RealSdk, String> {
        let mut names: Vec<String> = Vec::new();
        if let Ok(p) = std::env::var("RUSTFLARE_ASI_LIB") {
            names.push(p);
        }
        for n in ["libASICamera2.so", "libASICamera2.so.1", "libASICamera2.dylib"] {
            names.push(n.into());
        }
        let mut last = String::new();
        for n in &names {
            // SAFETY: loading a vendor shared library runs its initialisers; that is the point.
            match unsafe { Library::new(n) } {
                Ok(l) => return Self::bind(l),
                Err(e) => last = e.to_string(),
            }
        }
        Err(format!("{INSTALL_HINT} ({last})"))
    }

    fn bind(lib: Library) -> std::result::Result<RealSdk, String> {
        macro_rules! sym {
            ($n:literal, $t:ty) => {
                // SAFETY: the signatures match ASICamera2.h.
                *unsafe { lib.get::<$t>(concat!($n, "\0").as_bytes()) }
                    .map_err(|e| format!("libASICamera2 is missing {}: {e}", $n))?
            };
        }
        Ok(RealSdk {
            num: sym!("ASIGetNumOfConnectedCameras", FnVoidInt),
            info: sym!("ASIGetCameraProperty", FnInfo),
            open: sym!("ASIOpenCamera", FnId),
            init: sym!("ASIInitCamera", FnId),
            close: sym!("ASICloseCamera", FnId),
            num_controls: sym!("ASIGetNumOfControls", FnNum),
            caps: sym!("ASIGetControlCaps", FnCaps),
            get: sym!("ASIGetControlValue", FnGet),
            set: sym!("ASISetControlValue", FnSet),
            roi: sym!("ASISetROIFormat", FnRoi),
            start_pos: sym!("ASISetStartPos", FnStart),
            start: sym!("ASIStartVideoCapture", FnId),
            stop: sym!("ASIStopVideoCapture", FnId),
            data: sym!("ASIGetVideoData", FnData),
            lock: Mutex::new(()),
            _lib: lib,
        })
    }
}

fn rc(c: c_int) -> std::result::Result<(), i32> {
    if c == ASI_SUCCESS { Ok(()) } else { Err(c) }
}

impl Sdk for RealSdk {
    fn num_cameras(&self) -> i32 {
        let _g = self.lock.lock();
        unsafe { (self.num)() }
    }
    fn property(&self, index: i32) -> std::result::Result<Prop, i32> {
        let _g = self.lock.lock();
        // SAFETY: RawInfo is plain old data; all-zero is valid.
        let mut i: RawInfo = unsafe { std::mem::zeroed() };
        rc(unsafe { (self.info)(&mut i, index) })?;
        Ok(Prop {
            name: cstr(&i.name),
            id: i.camera_id,
            max_w: i.max_width as i32,
            max_h: i.max_height as i32,
            is_color: i.is_color != 0,
            bayer: i.bayer,
            pixel_size_um: i.pixel_size,
        })
    }
    fn open(&self, id: i32) -> std::result::Result<(), i32> {
        let _g = self.lock.lock();
        rc(unsafe { (self.open)(id) })
    }
    fn init(&self, id: i32) -> std::result::Result<(), i32> {
        let _g = self.lock.lock();
        rc(unsafe { (self.init)(id) })
    }
    fn close(&self, id: i32) {
        let _g = self.lock.lock();
        unsafe { (self.close)(id) };
    }
    fn caps(&self, id: i32) -> std::result::Result<Vec<Caps>, i32> {
        let _g = self.lock.lock();
        let mut n: c_int = 0;
        rc(unsafe { (self.num_controls)(id, &mut n) })?;
        let mut v = Vec::new();
        for k in 0..n {
            let mut c: RawCaps = unsafe { std::mem::zeroed() };
            rc(unsafe { (self.caps)(id, k, &mut c) })?;
            v.push(Caps {
                control_type: c.control_type,
                min: c.min as i64,
                max: c.max as i64,
                default: c.default as i64,
                writable: c.writable != 0,
            });
        }
        Ok(v)
    }
    fn get_control(&self, id: i32, ctl: i32) -> std::result::Result<i64, i32> {
        let _g = self.lock.lock();
        let (mut v, mut auto): (c_long, c_int) = (0, 0);
        rc(unsafe { (self.get)(id, ctl, &mut v, &mut auto) })?;
        Ok(v as i64)
    }
    fn set_control(&self, id: i32, ctl: i32, v: i64) -> std::result::Result<(), i32> {
        let _g = self.lock.lock();
        rc(unsafe { (self.set)(id, ctl, v as c_long, 0) })
    }
    fn set_format(&self, id: i32, w: i32, h: i32, img: i32) -> std::result::Result<(), i32> {
        let _g = self.lock.lock();
        rc(unsafe { (self.roi)(id, w, h, 1, img) })
    }
    fn set_start_pos(&self, id: i32, x: i32, y: i32) -> std::result::Result<(), i32> {
        let _g = self.lock.lock();
        rc(unsafe { (self.start_pos)(id, x, y) })
    }
    fn start_video(&self, id: i32) -> std::result::Result<(), i32> {
        let _g = self.lock.lock();
        rc(unsafe { (self.start)(id) })
    }
    fn stop_video(&self, id: i32) {
        let _g = self.lock.lock();
        unsafe { (self.stop)(id) };
    }
    fn video_data(&self, id: i32, buf: &mut [u8], wait_ms: i32) -> std::result::Result<(), i32> {
        // Not under the global lock: this blocks for up to a full exposure.
        rc(unsafe { (self.data)(id, buf.as_mut_ptr(), buf.len() as c_long, wait_ms) })
    }
}

/// The loaded library (or the reason it could not be loaded), shared by all cameras.
fn sdk() -> std::result::Result<Arc<dyn Sdk>, String> {
    static SDK: OnceLock<std::result::Result<Arc<RealSdk>, String>> = OnceLock::new();
    match SDK.get_or_init(|| RealSdk::load().map(Arc::new)) {
        Ok(s) => Ok(s.clone() as Arc<dyn Sdk>),
        Err(e) => Err(e.clone()),
    }
}

// ------------------------------------------------------------------ driver

pub struct AsiDriver;

impl CameraDriver for AsiDriver {
    fn name(&self) -> &'static str { DRIVER }

    fn enumerate(&self) -> Result<Vec<CameraInfo>> {
        match sdk() {
            Ok(s) => enumerate_with(&*s),
            Err(e) => {
                // Not an error for users without ZWO cameras: say it once, list nothing.
                static WARNED: OnceLock<()> = OnceLock::new();
                WARNED.get_or_init(|| log::info!("ASI driver disabled: {e}"));
                Ok(vec![])
            }
        }
    }

    fn open(&self, id: &str) -> Result<Box<dyn Camera>> {
        let s = sdk().map_err(Error::Camera)?;
        Ok(Box::new(open_with(s, id)?))
    }
}

fn info_of(p: &Prop) -> CameraInfo {
    CameraInfo { driver: DRIVER.into(), id: p.id.to_string(), name: p.name.clone() }
}

fn enumerate_with(s: &dyn Sdk) -> Result<Vec<CameraInfo>> {
    let n = s.num_cameras();
    let mut v = Vec::new();
    for i in 0..n {
        v.push(info_of(&s.property(i).map_err(|c| err("get camera property", c))?));
    }
    Ok(v)
}

fn open_with(s: Arc<dyn Sdk>, id: &str) -> Result<AsiCam> {
    let want: i32 = id.parse().map_err(|_| Error::Invalid(format!("camera id {id}")))?;
    let prop = (0..s.num_cameras())
        .filter_map(|i| s.property(i).ok())
        .find(|p| p.id == want)
        .ok_or_else(|| Error::NotFound(format!("ASI camera {id}")))?;
    s.open(want).map_err(|c| err("open (is another program using the camera?)", c))?;
    let setup = (|| {
        s.init(want).map_err(|c| err("init", c))?;
        s.set_format(want, prop.max_w, prop.max_h, IMG_RAW16).map_err(|c| err("set format", c))?;
        let caps = s.caps(want).map_err(|c| err("controls", c))?;
        // Use the full USB bandwidth by default (the SDK default can throttle USB3 cameras).
        if let Some(b) = caps.iter().find(|c| c.control_type == ASI_BANDWIDTHOVERLOAD && c.writable) {
            let _ = s.set_control(want, ASI_BANDWIDTHOVERLOAD, b.max);
        }
        Ok(caps)
    })();
    match setup {
        Ok(caps) => {
            let roi = Roi { x: 0, y: 0, width: prop.max_w as u32, height: prop.max_h as u32 };
            Ok(AsiCam { sdk: s, prop, caps, bits16: true, streaming: false, roi })
        }
        Err(e) => {
            s.close(want);
            Err(e)
        }
    }
}

struct AsiCam {
    sdk: Arc<dyn Sdk>,
    prop: Prop,
    caps: Vec<Caps>,
    bits16: bool,
    streaming: bool,
    /// The readout window now (the whole sensor until a region of interest is set).
    roi: Roi,
}

impl AsiCam {
    fn bayer_pattern(&self) -> Option<rustflare_core::BayerPattern> {
        use rustflare_core::BayerPattern as B;
        if !self.prop.is_color {
            return None;
        }
        Some(match self.prop.bayer { 1 => B::Bggr, 2 => B::Grbg, 3 => B::Gbrg, _ => B::Rggb })
    }

    fn stop_stream(&mut self) {
        if self.streaming {
            self.sdk.stop_video(self.prop.id);
            self.streaming = false;
        }
    }
    fn cap(&self, ctl: i32) -> Option<&Caps> {
        self.caps.iter().find(|c| c.control_type == ctl)
    }
}

impl Drop for AsiCam {
    fn drop(&mut self) {
        self.stop_stream();
        self.sdk.close(self.prop.id);
    }
}

impl Camera for AsiCam {
    fn bayer(&self) -> Option<rustflare_core::BayerPattern> { self.bayer_pattern() }

    fn info(&self) -> CameraInfo { info_of(&self.prop) }

    fn controls(&mut self) -> Result<Vec<ControlDesc>> {
        let id = self.prop.id;
        let mut out = Vec::new();
        let mut add = |cid: &str, label: &str, ctl: i32, unit: Option<&str>, log: bool, this: &AsiCam| -> Result<()> {
            if let Some(c) = this.cap(ctl) {
                let v = this.sdk.get_control(id, ctl).map_err(|e| err(label, e))?;
                out.push(ControlDesc {
                    id: cid.into(), label: label.into(), min: c.min as f64, max: c.max as f64, step: 1.0,
                    value: v as f64, unit: unit.map(String::from), log,
                });
            }
            Ok(())
        };
        add(CTRL_EXPOSURE_US, "Exposure", ASI_EXPOSURE, Some("µs"), true, self)?;
        add(CTRL_GAIN, "Gain", ASI_GAIN, None, false, self)?;
        add(CTRL_OFFSET, "Offset", ASI_OFFSET, None, false, self)?;
        add(CTRL_USB_BANDWIDTH, "USB bandwidth", ASI_BANDWIDTHOVERLOAD, Some("%"), false, self)?;
        out.push(ControlDesc {
            id: CTRL_HIGH_BIT_DEPTH.into(), label: "16-bit output".into(), min: 0.0, max: 1.0, step: 1.0,
            value: self.bits16 as i32 as f64, unit: None, log: false,
        });
        Ok(out)
    }

    fn set_control(&mut self, id: &str, value: f64) -> Result<()> {
        let ctl = match id {
            CTRL_EXPOSURE_US => ASI_EXPOSURE,
            CTRL_GAIN => ASI_GAIN,
            CTRL_OFFSET => ASI_OFFSET,
            CTRL_USB_BANDWIDTH => ASI_BANDWIDTHOVERLOAD,
            CTRL_HIGH_BIT_DEPTH => -1,
            _ => return Err(Error::Invalid(format!("unknown control {id}"))),
        };
        self.stop_stream(); // settings apply with the stream stopped; capture() restarts it
        let cid = self.prop.id;
        if ctl < 0 {
            let want16 = value >= 0.5;
            let img = if want16 { IMG_RAW16 } else { IMG_RAW8 };
            self.sdk.set_format(cid, self.roi.width as i32, self.roi.height as i32, img).map_err(|c| err("set format", c))?;
            // setting the format re-centres the window: put it back where it was
            self.sdk.set_start_pos(cid, self.roi.x as i32, self.roi.y as i32).map_err(|c| err("set start position", c))?;
            self.bits16 = want16;
            return Ok(());
        }
        let c = self.cap(ctl).ok_or_else(|| Error::Unsupported(format!("control {id} on this camera")))?;
        let v = (value.round() as i64).clamp(c.min, c.max);
        self.sdk.set_control(cid, ctl, v).map_err(|e| err(id, e))
    }

    fn sensor_size(&self) -> Option<(u32, u32)> { Some((self.prop.max_w as u32, self.prop.max_h as u32)) }

    fn roi(&self) -> Option<Roi> { Some(self.roi) }

    /// The SDK wants a width that is a multiple of 8 and a height that is a multiple of 2, a window inside the sensor, and an even corner
    /// (an odd one would swap the colours of a one-shot-colour camera). `ASISetROIFormat` re-centres the window, so the corner is set after it.
    fn set_roi(&mut self, roi: Roi) -> Result<()> {
        let (mw, mh) = (self.prop.max_w as u32, self.prop.max_h as u32);
        let (w, h) = (roi.width / 8 * 8, roi.height / 2 * 2);
        let (x, y) = (roi.x / 2 * 2, roi.y / 2 * 2);
        if w == 0 || h == 0 || x + w > mw || y + h > mh {
            return Err(Error::Invalid("region outside the sensor".into()));
        }
        self.stop_stream();
        let cid = self.prop.id;
        let img = if self.bits16 { IMG_RAW16 } else { IMG_RAW8 };
        self.sdk.set_format(cid, w as i32, h as i32, img).map_err(|c| err("set region size", c))?;
        self.sdk.set_start_pos(cid, x as i32, y as i32).map_err(|c| err("set region position", c))?;
        self.roi = Roi { x, y, width: w, height: h };
        Ok(())
    }

    fn capture(&mut self, timeout: Duration) -> Result<Frame> {
        let (w, h) = (self.roi.width as usize, self.roi.height as usize);
        let (pf, bpp) = if self.bits16 { (PixelFormat::Mono16, 2) } else { (PixelFormat::Mono8, 1) };
        if !self.streaming {
            self.sdk.start_video(self.prop.id).map_err(|c| err("start video capture", c))?;
            self.streaming = true;
        }
        let mut buf = vec![0u8; w * h * bpp];
        match self.sdk.video_data(self.prop.id, &mut buf, timeout.as_millis().min(i32::MAX as u128) as i32) {
            Ok(()) => {
                let mut f = Frame::new(w as u32, h as u32, pf, buf);
                f.bayer = self.bayer_pattern();
                Ok(f)
            }
            Err(c) => {
                if c != ASI_ERROR_TIMEOUT {
                    self.stop_stream(); // re-arm on the next call
                }
                Err(err("get video data", c))
            }
        }
    }

    fn temperature_c(&mut self) -> Option<f64> {
        self.cap(ASI_TEMPERATURE)?;
        // The SDK reports ten times the temperature.
        self.sdk.get_control(self.prop.id, ASI_TEMPERATURE).ok().map(|v| v as f64 / 10.0)
    }

    fn pixel_size_um(&self) -> Option<f64> {
        (self.prop.pixel_size_um > 0.0).then_some(self.prop.pixel_size_um)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Fake {
        state: Mutex<FakeState>,
        color: bool,
        big: bool,
    }
    #[derive(Default)]
    struct FakeState {
        values: HashMap<i32, i64>,
        img: i32,
        streaming: bool,
        starts: u32,
        closed: bool,
        fail_open: bool,
        fail_data: Option<i32>,
        window: (i32, i32, i32, i32),
    }
    impl Sdk for Fake {
        fn num_cameras(&self) -> i32 { 1 }
        fn property(&self, i: i32) -> std::result::Result<Prop, i32> {
            if i != 0 { return Err(1); }
            let (max_w, max_h) = if self.big { (64, 32) } else { (4, 2) };
            Ok(Prop { name: "ZWO ASI174MM Mini".into(), id: 0, max_w, max_h, is_color: self.color, bayer: 0, pixel_size_um: 5.86 })
        }
        fn open(&self, _: i32) -> std::result::Result<(), i32> {
            if self.state.lock().unwrap().fail_open { Err(5) } else { Ok(()) }
        }
        fn init(&self, _: i32) -> std::result::Result<(), i32> { Ok(()) }
        fn close(&self, _: i32) { self.state.lock().unwrap().closed = true; }
        fn caps(&self, _: i32) -> std::result::Result<Vec<Caps>, i32> {
            let c = |t, min, max, writable| Caps { control_type: t, min, max, default: min, writable };
            Ok(vec![c(ASI_GAIN, 0, 510, true), c(ASI_EXPOSURE, 32, 2_000_000_000, true), c(ASI_OFFSET, 0, 80, true),
                    c(ASI_BANDWIDTHOVERLOAD, 40, 100, true), c(ASI_TEMPERATURE, -500, 1000, false)])
        }
        fn get_control(&self, _: i32, ctl: i32) -> std::result::Result<i64, i32> {
            Ok(*self.state.lock().unwrap().values.get(&ctl).unwrap_or(&if ctl == ASI_TEMPERATURE { 253 } else { 7 }))
        }
        fn set_control(&self, _: i32, ctl: i32, v: i64) -> std::result::Result<(), i32> {
            self.state.lock().unwrap().values.insert(ctl, v);
            Ok(())
        }
        fn set_format(&self, _: i32, w: i32, h: i32, img: i32) -> std::result::Result<(), i32> {
            let mut s = self.state.lock().unwrap();
            if s.streaming { return Err(12); }
            s.img = img;
            s.window = (0, 0, w, h); // the real library centres the window here
            Ok(())
        }
        fn set_start_pos(&self, _: i32, x: i32, y: i32) -> std::result::Result<(), i32> {
            let mut s = self.state.lock().unwrap();
            if s.streaming { return Err(12); }
            s.window.0 = x;
            s.window.1 = y;
            Ok(())
        }
        fn start_video(&self, _: i32) -> std::result::Result<(), i32> {
            let mut s = self.state.lock().unwrap();
            s.streaming = true;
            s.starts += 1;
            Ok(())
        }
        fn stop_video(&self, _: i32) { self.state.lock().unwrap().streaming = false; }
        fn video_data(&self, _: i32, buf: &mut [u8], _: i32) -> std::result::Result<(), i32> {
            let s = self.state.lock().unwrap();
            if let Some(c) = s.fail_data { return Err(c); }
            for (i, b) in buf.iter_mut().enumerate() { *b = i as u8; }
            Ok(())
        }
    }

    fn cam(f: &Arc<Fake>) -> AsiCam { open_with(f.clone() as Arc<dyn Sdk>, "0").unwrap() }

    #[test]
    fn enumerates_and_reports_pixel_size() {
        let f = Arc::new(Fake::default());
        let l = enumerate_with(&*f).unwrap();
        assert_eq!((l[0].driver.as_str(), l[0].id.as_str()), ("asi", "0"));
        assert_eq!(cam(&f).pixel_size_um(), Some(5.86));
    }

    #[test]
    fn controls_use_standard_ids_and_ranges() {
        let f = Arc::new(Fake::default());
        let c = cam(&f).controls().unwrap();
        let ids: Vec<&str> = c.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, [CTRL_EXPOSURE_US, CTRL_GAIN, CTRL_OFFSET, CTRL_USB_BANDWIDTH, CTRL_HIGH_BIT_DEPTH]);
        assert_eq!(c[0].max, 2_000_000_000.0);
        assert_eq!(c[4].value, 1.0); // opens in 16-bit
        // bandwidth raised to the maximum on open
        assert_eq!(f.state.lock().unwrap().values[&ASI_BANDWIDTHOVERLOAD], 100);
    }

    #[test]
    fn set_control_clamps_and_rejects_unknown() {
        let f = Arc::new(Fake::default());
        let mut c = cam(&f);
        c.set_control(CTRL_GAIN, 9999.0).unwrap();
        assert_eq!(f.state.lock().unwrap().values[&ASI_GAIN], 510);
        assert!(matches!(c.set_control("bogus", 1.0), Err(Error::Invalid(_))));
    }

    #[test]
    fn capture_formats_and_streams_once() {
        let f = Arc::new(Fake::default());
        let mut c = cam(&f);
        let t = Duration::from_millis(10);
        let fr = c.capture(t).unwrap();
        assert_eq!((fr.width, fr.height, fr.format), (4, 2, PixelFormat::Mono16));
        assert_eq!(fr.data.len(), 16);
        c.capture(t).unwrap();
        assert_eq!(f.state.lock().unwrap().starts, 1); // armed once
        // a setting change stops the stream (format change would fail while streaming) and re-arms
        c.set_control(CTRL_HIGH_BIT_DEPTH, 0.0).unwrap();
        let fr = c.capture(t).unwrap();
        assert_eq!((fr.format, fr.data.len()), (PixelFormat::Mono8, 8));
        assert_eq!(f.state.lock().unwrap().starts, 2);
    }

    #[test]
    fn a_region_of_interest_is_read_out_and_survives_a_change_of_bit_depth() {
        let f = Arc::new(Fake { big: true, ..Default::default() });
        let mut c = cam(&f);
        assert_eq!(c.sensor_size(), Some((64, 32)));
        assert_eq!(c.roi(), Some(Roi { x: 0, y: 0, width: 64, height: 32 }));
        // the size is rounded down to what the SDK takes (width x8, height x2), the corner to even numbers
        c.set_roi(Roi { x: 13, y: 7, width: 30, height: 15 }).unwrap();
        assert_eq!(c.roi(), Some(Roi { x: 12, y: 6, width: 24, height: 14 }));
        assert_eq!(f.state.lock().unwrap().window, (12, 6, 24, 14));
        let fr = c.capture(Duration::from_millis(5)).unwrap();
        assert_eq!((fr.width, fr.height, fr.data.len()), (24, 14, 24 * 14 * 2));
        // 8-bit again: the window stays where it was (the SDK re-centres it on every format call)
        c.set_control(CTRL_HIGH_BIT_DEPTH, 0.0).unwrap();
        assert_eq!(f.state.lock().unwrap().window, (12, 6, 24, 14));
        assert_eq!(c.capture(Duration::from_millis(5)).unwrap().data.len(), 24 * 14);
        // outside the sensor, or empty
        assert!(c.set_roi(Roi { x: 60, y: 0, width: 24, height: 14 }).is_err());
        assert!(c.set_roi(Roi { x: 0, y: 0, width: 4, height: 14 }).is_err());
        // back to the whole sensor
        c.set_roi(Roi { x: 0, y: 0, width: 64, height: 32 }).unwrap();
        assert_eq!(c.capture(Duration::from_millis(5)).unwrap().width, 64);
    }

    #[test]
    fn timeout_keeps_stream_other_errors_rearm() {
        let f = Arc::new(Fake::default());
        let mut c = cam(&f);
        f.state.lock().unwrap().fail_data = Some(ASI_ERROR_TIMEOUT);
        assert!(c.capture(Duration::from_millis(1)).is_err());
        assert!(f.state.lock().unwrap().streaming);
        f.state.lock().unwrap().fail_data = Some(5);
        assert!(c.capture(Duration::from_millis(1)).is_err());
        assert!(!f.state.lock().unwrap().streaming);
    }

    #[test]
    fn temperature_is_tenths_of_a_degree() {
        let f = Arc::new(Fake::default());
        assert_eq!(cam(&f).temperature_c(), Some(25.3));
    }

    #[test]
    fn open_errors_and_close_on_drop() {
        let f = Arc::new(Fake::default());
        assert!(matches!(open_with(f.clone(), "x"), Err(Error::Invalid(_))));
        assert!(matches!(open_with(f.clone(), "3"), Err(Error::NotFound(_))));
        f.state.lock().unwrap().fail_open = true;
        assert!(open_with(f.clone(), "0").is_err());
        f.state.lock().unwrap().fail_open = false;
        drop(cam(&f));
        assert!(f.state.lock().unwrap().closed);
        let colour = Arc::new(Fake { color: true, ..Default::default() });
        let mut c = open_with(colour, "0").unwrap();
        assert_eq!(c.bayer(), Some(rustflare_core::BayerPattern::Rggb));
        assert_eq!(c.capture(Duration::from_secs(1)).unwrap().bayer, Some(rustflare_core::BayerPattern::Rggb));
    }

    #[test]
    fn missing_library_is_friendly() {
        std::env::set_var("RUSTFLARE_ASI_LIB", "/nonexistent/libASICamera2.so");
        // On a machine with libasi this succeeds; otherwise the message must carry the install hint.
        if let Err(e) = AsiDriver.open("0") {
            assert!(e.to_string().contains("sudo apt install libasi"));
        }
        assert!(AsiDriver.enumerate().is_ok());
    }
}
