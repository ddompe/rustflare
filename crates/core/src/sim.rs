//! Shared state linking the simulated mount and simulated camera, so that
//! guiding / calibration can be exercised end-to-end without hardware.

use crate::Vec2;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Image-space pixels per second moved at 1x sidereal rate.
pub const PX_PER_SIDEREAL: f64 = 15.0;
/// Residual sky drift (px/s) while tracking; simulates imperfect polar alignment.
pub const TRACKING_DRIFT: Vec2 = Vec2::new(0.4, -0.25);
/// Un-tracked drift (px/s).
pub const FREE_DRIFT: Vec2 = Vec2::new(-15.0, 0.0);

/// Direction in image space of positive motion on each mount axis (deliberately
/// rotated so that calibration has something to discover).
pub fn axis_dirs() -> (Vec2, Vec2) {
    let a = 30f64.to_radians();
    (Vec2::new(a.cos(), a.sin()), Vec2::new(-a.sin(), a.cos()))
}

#[derive(Debug)]
struct State {
    offset: Vec2,
    last: Instant,
    tracking: bool,
    /// Commanded motion on [primary, secondary] in multiples of sidereal (signed).
    axis_rate: [f64; 2],
}

#[derive(Clone, Debug)]
pub struct SimSky(Arc<Mutex<State>>);

impl SimSky {
    pub fn new() -> SimSky {
        SimSky(Arc::new(Mutex::new(State {
            offset: Vec2::default(),
            last: Instant::now(),
            tracking: false,
            axis_rate: [0.0; 2],
        })))
    }

    fn advance(s: &mut State) {
        let now = Instant::now();
        let dt = now.duration_since(s.last).as_secs_f64();
        s.last = now;
        let (p, q) = axis_dirs();
        let mut v = if s.tracking { TRACKING_DRIFT } else { FREE_DRIFT };
        v = v + p * (s.axis_rate[0] * PX_PER_SIDEREAL) + q * (s.axis_rate[1] * PX_PER_SIDEREAL);
        s.offset = s.offset + v * dt;
    }

    pub fn set_tracking(&self, on: bool) {
        let mut s = self.0.lock().unwrap();
        Self::advance(&mut s);
        s.tracking = on;
    }

    pub fn set_axis_rates(&self, primary: f64, secondary: f64) {
        let mut s = self.0.lock().unwrap();
        Self::advance(&mut s);
        s.axis_rate = [primary, secondary];
    }

    /// Puts the image back at the frame centre (a new simulated scene starts there).
    pub fn recentre(&self) {
        let mut s = self.0.lock().unwrap();
        Self::advance(&mut s);
        s.offset = Vec2::default();
    }

    /// Current displacement of the Sun image from frame centre, in pixels.
    pub fn offset(&self) -> Vec2 {
        let mut s = self.0.lock().unwrap();
        Self::advance(&mut s);
        s.offset
    }
}

impl Default for SimSky {
    fn default() -> Self { Self::new() }
}
