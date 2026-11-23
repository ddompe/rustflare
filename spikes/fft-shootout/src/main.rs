use realfft::RealFftPlanner;
use rustfft::{num_complex::Complex, FftPlanner};
use std::ffi::c_void;
use std::time::Instant;

type C = Complex<f32>;

extern "C" {
    fn pffft_new_setup(n: i32, t: i32) -> *mut c_void;
    fn pffft_destroy_setup(s: *mut c_void);
    fn pffft_transform_ordered(s: *mut c_void, input: *const f32, output: *mut f32, work: *mut f32, dir: i32);
    fn pffft_aligned_malloc(n: usize) -> *mut c_void;
    fn pffft_aligned_free(p: *mut c_void);
}
const PFFFT_COMPLEX: i32 = 1;
const FORWARD: i32 = 0;
const BACKWARD: i32 = 1;

fn naive_transpose(src: &[C], w: usize, h: usize) -> Vec<C> {
    let mut o = vec![C::new(0.0, 0.0); w * h];
    for y in 0..h { for x in 0..w { o[x * h + y] = src[y * w + x]; } }
    o
}

fn blocked_transpose<T: Copy + Default>(src: &[T], w: usize, h: usize, out: &mut [T]) {
    const B: usize = 16;
    for by in (0..h).step_by(B) {
        for bx in (0..w).step_by(B) {
            for y in by..(by + B).min(h) { for x in bx..(bx + B).min(w) { out[x * h + y] = src[y * w + x]; } }
        }
    }
}

/// Complex 2D FFT as the stacker does it now: rows, naive transpose, rows. Result in [w rows][h cols] layout.
fn rustfft_naive(w: usize, h: usize, input: &[C], inverse: bool) -> Vec<C> {
    let mut p = FftPlanner::<f32>::new();
    let (fw, fh) = if inverse { (p.plan_fft_inverse(w), p.plan_fft_inverse(h)) } else { (p.plan_fft_forward(w), p.plan_fft_forward(h)) };
    let mut buf = input.to_vec();
    for r in buf.chunks_exact_mut(w) { fw.process(r); }
    let mut t = naive_transpose(&buf, w, h);
    for r in t.chunks_exact_mut(h) { fh.process(r); }
    t
}

struct Rf { fw: std::sync::Arc<dyn rustfft::Fft<f32>>, fh: std::sync::Arc<dyn rustfft::Fft<f32>>, w: usize, h: usize, t: Vec<C> }
impl Rf {
    fn new(w: usize, h: usize) -> Rf { let mut p = FftPlanner::<f32>::new(); Rf { fw: p.plan_fft_forward(w), fh: p.plan_fft_forward(h), w, h, t: vec![C::new(0.0, 0.0); w * h] } }
    fn run(&mut self, buf: &mut [C]) {
        for r in buf.chunks_exact_mut(self.w) { self.fw.process(r); }
        blocked_transpose(buf, self.w, self.h, &mut self.t);
        for r in self.t.chunks_exact_mut(self.h) { self.fh.process(r); }
    }
}

/// Real-to-complex 2D: rows r2c (w/2+1 outputs), blocked transpose, complex columns. Half the work for real images.
struct Real2d { r2c: std::sync::Arc<dyn realfft::RealToComplex<f32>>, fh: std::sync::Arc<dyn rustfft::Fft<f32>>, w: usize, h: usize, half: Vec<C>, t: Vec<C>, scratch: Vec<C> }
impl Real2d {
    fn new(w: usize, h: usize) -> Real2d {
        let mut rp = RealFftPlanner::<f32>::new(); let r2c = rp.plan_fft_forward(w);
        let mut p = FftPlanner::<f32>::new();
        let s = r2c.get_scratch_len();
        Real2d { r2c, fh: p.plan_fft_forward(h), w, h, half: vec![C::new(0.0, 0.0); (w / 2 + 1) * h], t: vec![C::new(0.0, 0.0); (w / 2 + 1) * h], scratch: vec![C::new(0.0, 0.0); s] }
    }
    fn run(&mut self, img: &mut [f32]) {
        let hw = self.w / 2 + 1;
        for (y, row) in img.chunks_exact_mut(self.w).enumerate() { self.r2c.process_with_scratch(row, &mut self.half[y * hw..(y + 1) * hw], &mut self.scratch).unwrap(); }
        blocked_transpose(&self.half, hw, self.h, &mut self.t);
        for r in self.t.chunks_exact_mut(self.h) { self.fh.process(r); }
    }
}

struct Pf { sw: *mut c_void, sh: *mut c_void, w: usize, h: usize, a: *mut f32, b: *mut f32, work: *mut f32, t: Vec<C> }
impl Pf {
    fn new(w: usize, h: usize) -> Pf { unsafe {
        let n = w * h * 2 * 4;
        Pf { sw: pffft_new_setup(w as i32, PFFFT_COMPLEX), sh: pffft_new_setup(h as i32, PFFFT_COMPLEX), w, h, a: pffft_aligned_malloc(n) as *mut f32, b: pffft_aligned_malloc(n) as *mut f32, work: pffft_aligned_malloc(w.max(h) * 2 * 4) as *mut f32, t: vec![C::new(0.0, 0.0); w * h] } } }
    fn load(&mut self, input: &[C]) { unsafe { std::ptr::copy_nonoverlapping(input.as_ptr() as *const f32, self.a, self.w * self.h * 2); } }
    fn run(&mut self, dir: i32) { unsafe {
        for y in 0..self.h { pffft_transform_ordered(self.sw, self.a.add(y * self.w * 2), self.b.add(y * self.w * 2), self.work, dir); }
        let b = std::slice::from_raw_parts(self.b as *const C, self.w * self.h);
        blocked_transpose(b, self.w, self.h, &mut self.t);
        let t = self.t.as_ptr() as *const f32;
        for y in 0..self.w { pffft_transform_ordered(self.sh, t.add(y * self.h * 2), self.a.add(y * self.h * 2), self.work, dir); }
    } }
    fn result(&self) -> Vec<C> { unsafe { std::slice::from_raw_parts(self.a as *const C, self.w * self.h).to_vec() } }
}
impl Drop for Pf { fn drop(&mut self) { unsafe { pffft_destroy_setup(self.sw); pffft_destroy_setup(self.sh); pffft_aligned_free(self.a as _); pffft_aligned_free(self.b as _); pffft_aligned_free(self.work as _); } } }

fn bench(label: &str, n: usize, mut f: impl FnMut()) -> f64 {
    f(); let t = Instant::now(); for _ in 0..n { f(); } let ms = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
    println!("    {label:52} {ms:9.3} ms"); ms
}

fn main() {
    for (w, h, wp, hp, n, name) in [(480usize, 360usize, 480usize, 384usize, 40usize, "global alignment 480x360 (PFFFT needs multiples of 16: 480x384)"), (80, 80, 80, 80, 4000, "per-point correlation 80x80")] {
        println!("{name}: one 2D FFT forward, complex or real input");
        let mut s = 12345u32;
        let mut rnd = || { s = s.wrapping_mul(1664525).wrapping_add(1013904223); (s >> 8) as f32 / 16777216.0 - 0.5 };
        let img: Vec<f32> = (0..w * h).map(|_| rnd()).collect();
        let cimg: Vec<C> = img.iter().map(|&v| C::new(v, 0.0)).collect();
        let base = bench("A rustfft, naive transpose (current code)", n, || { std::hint::black_box(rustfft_naive(w, h, &cimg, false)); });
        let mut rf = Rf::new(w, h);
        let mut buf = cimg.clone();
        let b = bench("B rustfft, blocked transpose", n, || { buf.copy_from_slice(&cimg); rf.run(&mut buf); });
        let mut r2 = Real2d::new(w, h);
        let mut im = img.clone();
        let c = bench("C realfft real-to-complex (half spectrum), blocked transpose", n, || { im.copy_from_slice(&img); r2.run(&mut im); });
        let pf_img: Vec<C> = { let mut v = vec![C::new(0.0, 0.0); wp * hp]; for y in 0..h.min(hp) { for x in 0..w.min(wp) { v[y * wp + x] = cimg[y * w + x]; } } v };
        let mut pf = Pf::new(wp, hp);
        pf.load(&pf_img);
        let d = bench(&format!("D PFFFT complex {wp}x{hp}, blocked transpose"), n, || { pf.load(&pf_img); pf.run(FORWARD); });
        println!("    -> B {:.2}x, C {:.2}x, D {:.2}x of A", base / b, base / c, base / d);
        // correctness: B and D (when the size is the same) against A
        let a = rustfft_naive(w, h, &cimg, false);
        let mut bb = cimg.clone(); rf.run(&mut bb);
        let err = |x: &[C], y: &[C]| x.iter().zip(y).map(|(p, q)| (p - q).norm()).fold(0.0f32, f32::max);
        println!("    check: B vs A max abs difference {:.2e}", err(&rf.t, &a));
        if (w, h) == (wp, hp) { pf.load(&pf_img); pf.run(FORWARD); println!("    check: D vs A max abs difference {:.2e}", err(&pf.result(), &a)); }
        let _ = (BACKWARD, bb);
        println!();
    }
}
