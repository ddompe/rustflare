//! What the detector and the tracker make of the simulated Moon and planet scenes (use profiles, stage 3).
//! These ran first as an experiment: the proposal inferred from the code that a 25 px planet is not found.

use rustflare_camera::simulator::SimulatorDriver;
use rustflare_core::camera::CameraDriver;
use rustflare_core::sim::SimSky;
use rustflare_core::Frame;
use rustflare_tracking::{locate_planet, locate_sun, SunLocation};
use std::time::{Duration, Instant};

pub fn scene(id: u8, controls: &[(&str, f64)]) -> Frame {
    let sky = SimSky::new();
    sky.set_tracking(true);
    let mut cam = SimulatorDriver::new(sky).open("sim0").unwrap();
    cam.set_control("sim_scene", id as f64).unwrap();
    cam.set_control("exposure_us", 2000.0).unwrap();
    for (k, v) in controls { cam.set_control(k, *v).unwrap(); }
    cam.capture(Duration::from_secs(1)).unwrap()
}

/// The claim in the proposal (section 4.1), run: "the detector very probably does not see a planet at all". CONFIRMED. On the simulated
/// 25 px planet `locate_sun` does not return the planet, it returns a confident phantom: a partial "Sun" of 440 px radius whose centre is
/// 90 px from the planet, fitted to the noise. This is why the planet profile does not use it (`engine::detect_target`).
#[test]
fn the_sun_detector_does_not_find_a_planet_and_the_planet_locator_does() {
    let p = scene(3, &[]);
    let (cx, cy) = (640.0, 480.0);
    for hint in [None, Some(25.0)] {
        let l = locate_sun(&p, hint).expect("it answers, wrongly");
        let off = ((l.pos.x - cx).powi(2) + (l.pos.y - cy).powi(2)).sqrt();
        assert!(off > 30.0 || l.radius > 100.0, "the Sun detector must not be mistaken for finding a 25 px planet: {l:?}");
    }
    let l = locate_planet(&p, None).expect("the planet locator finds it");
    assert!((l.pos.x - cx).abs() < 1.5 && (l.pos.y - cy).abs() < 1.5, "{l:?}");
    assert!((15.0..30.0).contains(&l.radius), "radius {}", l.radius);
}

/// The planet tracker and the one model, with seeing: the tracker follows within a pixel or two, and the model fed by it with the
/// planet profile's floor stays on the planet.
#[test]
fn the_planet_tracker_and_the_model_hold_a_planet_in_the_seeing() {
    let sky = SimSky::new();
    sky.set_tracking(true);
    let mut cam = SimulatorDriver::new(sky).open("sim0").unwrap();
    cam.set_control("sim_scene", 3.0).unwrap();
    cam.set_control("sim_seeing", 3.0).unwrap();
    let first = cam.capture(Duration::from_secs(1)).unwrap();
    let mut tracker = rustflare_tracking::Tracker::planet(&first).expect("a planet is in view");
    assert_eq!(tracker.mode(), "planet");
    let mut model = crate::sunmodel::SunModel::default();
    model.set_gate_floor(8.0);
    let t0 = Instant::now();
    let (mut worst, mut worst_model, mut accepted) = (0.0f64, 0.0f64, 0);
    for i in 0..40 {
        let f = cam.capture(Duration::from_secs(1)).unwrap();
        let p = tracker.update(&f).expect("the planet stays found under 3 px seeing");
        worst = worst.max(((p.pos.x - 640.0).powi(2) + (p.pos.y - 480.0).powi(2)).sqrt());
        let meas = SunLocation { pos: p.pos, radius: p.radius.unwrap(), partial: true, radius_from_optics: None, fit_rms: None };
        let e = model.update(Some(meas), t0 + Duration::from_millis(100 * i), (f.width, f.height), false, None).unwrap();
        if i >= 10 { worst_model = worst_model.max(((e.sun.pos.x - 640.0).powi(2) + (e.sun.pos.y - 480.0).powi(2)).sqrt()); if !e.predicted { accepted += 1; } }
    }
    assert!(worst < 6.0, "the centroid follows the shaken planet within the jitter: {worst}");
    assert!(worst_model < 4.0, "the model is steadier than the measurements: {worst_model}");
    assert!(accepted >= 28, "the model believes the measurements: {accepted} of 30");
}

/// What the Sun detector does with a Moon with a phase (characterisation, not a wish): a full Moon is a disc and is found exactly; with a
/// terminator it takes the lit part for the disc, so the centre moves toward the lit side and the radius shrinks. The Moon profile
/// therefore says in the Track tab that the terminator is not the limb, and the limb fit is for a mostly full Moon.
#[test]
fn the_moon_is_found_when_full_and_the_lit_part_is_taken_for_the_disc_otherwise() {
    let full = locate_sun(&scene(2, &[("sim_phase", 0.5)]), None).unwrap();
    assert!((full.pos.x - 640.0).abs() < 3.0 && (full.pos.y - 480.0).abs() < 3.0 && (full.radius - 300.0).abs() < 4.0, "{full:?}");
    let gibbous = locate_sun(&scene(2, &[("sim_phase", 0.75)]), None).unwrap();
    assert!((gibbous.pos.x - 640.0).abs() > 50.0 && gibbous.radius < 260.0, "the lit part, not the disc: {gibbous:?}");
}
