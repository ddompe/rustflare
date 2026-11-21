//! Minimal 2-D vector / matrix types (enough for guiding math).

use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

impl Vec2 {
    pub const fn new(x: f64, y: f64) -> Self {
        Vec2 { x, y }
    }
    pub fn len(&self) -> f64 {
        self.x.hypot(self.y)
    }
    pub fn normalized(&self) -> Option<Vec2> {
        let l = self.len();
        if l < 1e-12 { None } else { Some(Vec2::new(self.x / l, self.y / l)) }
    }
}

impl std::ops::Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 { Vec2::new(self.x - o.x, self.y - o.y) }
}
impl std::ops::Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 { Vec2::new(self.x + o.x, self.y + o.y) }
}
impl std::ops::Mul<f64> for Vec2 {
    type Output = Vec2;
    fn mul(self, k: f64) -> Vec2 { Vec2::new(self.x * k, self.y * k) }
}

/// Row-major 2x2 matrix.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mat2 {
    pub m: [[f64; 2]; 2],
}

impl Mat2 {
    /// Matrix whose columns are `c0` and `c1`.
    pub fn from_cols(c0: Vec2, c1: Vec2) -> Mat2 {
        Mat2 { m: [[c0.x, c1.x], [c0.y, c1.y]] }
    }
    pub fn invert(&self) -> Option<Mat2> {
        let [[a, b], [c, d]] = self.m;
        let det = a * d - b * c;
        if det.abs() < 1e-9 {
            return None;
        }
        Some(Mat2 { m: [[d / det, -b / det], [-c / det, a / det]] })
    }
    pub fn apply(&self, v: Vec2) -> Vec2 {
        Vec2::new(
            self.m[0][0] * v.x + self.m[0][1] * v.y,
            self.m[1][0] * v.x + self.m[1][1] * v.y,
        )
    }
}
