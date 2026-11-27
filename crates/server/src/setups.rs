//! Named optical setups ("optical trains"): a telescope with its reducer or Barlow, the pixel size and binning, the profile it is
//! used for and the flat and dark that belong to it. Kept in the settings file under `optical_setups` (an array, order kept) and
//! `optical_setup` (the active name). Nothing here touches a camera or a mount; the engine applies a setup through the same paths
//! as `PUT /api/optics` and `PUT /api/profile`.

use crate::engine::OpticsSettings;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

/// Prefix of the error for an unknown name: the API turns it into a 404.
pub const NOT_FOUND: &str = "no optical setup named";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct OpticalSetup {
    pub name: String,
    /// The default profile of this train (one of `status::PROFILES`); none means applying it leaves the profile alone.
    #[serde(default)]
    pub profile: Option<String>,
    pub focal_length_mm: Option<f64>,
    #[serde(default = "one_f")]
    pub focal_multiplier: f64,
    /// Pixel size override; none means the camera's own.
    #[serde(default)]
    pub pixel_size_um: Option<f64>,
    #[serde(default = "one")]
    pub binning: u32,
    /// Aperture, for the f-ratio.
    #[serde(default)]
    pub aperture_mm: Option<f64>,
    #[serde(default)]
    pub notes: Option<String>,
    /// Master flat and dark that belong to this train (paths as in `status.flats.name` / `status.darks.name`).
    #[serde(default)]
    pub flat: Option<String>,
    #[serde(default)]
    pub dark: Option<String>,
    /// The exposure and gain the dark was taken at, so re-selecting it through the setup still matches the camera.
    #[serde(default)]
    pub dark_exposure_us: Option<f64>,
    #[serde(default)]
    pub dark_gain: Option<f64>,
}
fn one() -> u32 { 1 }
fn one_f() -> f64 { 1.0 }

/// What is in use now: the setup is saved from it and compared with it.
#[derive(Clone, Debug)]
pub struct Current {
    pub optics: OpticsSettings,
    pub profile: String,
    pub flat: Option<String>,
    pub dark: Option<String>,
    pub dark_exposure_us: Option<f64>,
    pub dark_gain: Option<f64>,
}

/// The range checks of the optics, shared with `PUT /api/optics`.
pub fn check_optics(focal_length_mm: Option<f64>, focal_multiplier: f64, pixel_size_um: Option<f64>, binning: u32) -> Result<(), String> {
    if focal_length_mm.is_some_and(|f| !(10.0..=20000.0).contains(&f)) {
        return Err("focal length must be between 10 and 20000 mm".into());
    }
    if pixel_size_um.is_some_and(|p| !(0.5..=50.0).contains(&p)) {
        return Err("pixel size must be between 0.5 and 50 µm".into());
    }
    if !(0.2..=10.0).contains(&focal_multiplier) {
        return Err("Barlow/reducer factor must be between 0.2 and 10 (e.g. 2 for a 2x Barlow, 0.63 for a reducer)".into());
    }
    if !(1..=8).contains(&binning) {
        return Err("binning must be 1 to 8".into());
    }
    Ok(())
}

/// A name is trimmed, 1 to 40 characters, with no control characters (and no `/`, which cannot travel in the URL path).
pub fn clean_name(raw: &str) -> Result<String, String> {
    let n = raw.trim();
    if n.is_empty() {
        return Err("give the setup a name".into());
    }
    if n.chars().count() > 40 {
        return Err("the name is at most 40 characters".into());
    }
    if n.chars().any(|c| c.is_control()) {
        return Err("the name cannot contain control characters".into());
    }
    if n.contains('/') {
        return Err("the name cannot contain a slash".into());
    }
    Ok(n.to_string())
}

fn blank_to_none(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

impl OpticalSetup {
    /// Checks and normalises (trimmed name, blank notes and names become none).
    pub fn validate(mut self) -> Result<Self, String> {
        self.name = clean_name(&self.name)?;
        if let Some(p) = &self.profile {
            if !crate::status::PROFILES.contains(&p.as_str()) {
                return Err(format!("unknown profile {p} (use {})", crate::status::PROFILES.join(", ")));
            }
        }
        check_optics(self.focal_length_mm, self.focal_multiplier, self.pixel_size_um, self.binning)?;
        if self.aperture_mm.is_some_and(|a| !(5.0..=5000.0).contains(&a)) {
            return Err("aperture must be between 5 and 5000 mm".into());
        }
        self.notes = blank_to_none(self.notes);
        if self.notes.as_ref().is_some_and(|n| n.chars().count() > 500) {
            return Err("notes are at most 500 characters".into());
        }
        self.flat = blank_to_none(self.flat);
        self.dark = blank_to_none(self.dark);
        Ok(self)
    }

    pub fn optics(&self) -> OpticsSettings {
        OpticsSettings { focal_length_mm: self.focal_length_mm, focal_multiplier: self.focal_multiplier, pixel_size_um: self.pixel_size_um, binning: self.binning }
    }

    /// Does the current state differ from this setup? The profile counts only when the setup has one.
    pub fn differs_from(&self, cur: &Current) -> bool {
        let same = |a: Option<f64>, b: Option<f64>| match (a, b) { (None, None) => true, (Some(a), Some(b)) => (a - b).abs() < 1e-6, _ => false };
        !same(self.focal_length_mm, cur.optics.focal_length_mm)
            || (self.focal_multiplier - cur.optics.focal_multiplier).abs() > 1e-6
            || !same(self.pixel_size_um, cur.optics.pixel_size_um)
            || self.binning != cur.optics.binning
            || self.profile.as_ref().is_some_and(|p| *p != cur.profile)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Setups {
    pub list: Vec<OpticalSetup>,
    pub active: Option<String>,
}

impl Setups {
    pub fn load(file: &Path) -> Self {
        let cfg = crate::config::read(file);
        let list: Vec<OpticalSetup> = cfg
            .get("optical_setups")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| serde_json::from_value::<OpticalSetup>(v.clone()).ok()).filter_map(|s| s.validate().ok()).collect())
            .unwrap_or_default();
        let active = cfg.get("optical_setup").and_then(|v| v.as_str()).filter(|n| list.iter().any(|s| s.name == *n)).map(String::from);
        Setups { list, active }
    }

    pub fn persist(&self, file: &Path) -> Result<(), String> {
        crate::config::set(file, "optical_setups", serde_json::to_value(&self.list).unwrap_or_default())?;
        crate::config::set(file, "optical_setup", json!(self.active))
    }

    pub fn get(&self, name: &str) -> Result<&OpticalSetup, String> {
        let n = name.trim();
        self.list.iter().find(|s| s.name == n).ok_or_else(|| format!("{NOT_FOUND} {n}"))
    }

    pub fn active_setup(&self) -> Option<&OpticalSetup> {
        self.active.as_deref().and_then(|n| self.list.iter().find(|s| s.name == n))
    }

    /// The active setup has been changed since it was saved (current optics or profile differ from it). No setup: not modified.
    pub fn modified(&self, cur: &Current) -> bool {
        self.active_setup().is_some_and(|s| s.differs_from(cur))
    }

    /// Saves (creates or replaces, keeping the order of an existing one). Fields missing from `body` come from the current optics,
    /// profile, flat and dark; aperture and notes missing stay empty. Returns the saved setup. The saved setup becomes the active one
    /// when it equals what is in use now (nothing is overwritten by that), unless `body.activate` says false.
    pub fn save(&mut self, name: &str, body: &Value, cur: &Current) -> Result<OpticalSetup, String> {
        let name = clean_name(name)?;
        let obj = body.as_object().cloned().unwrap_or_default();
        let mut m = serde_json::Map::new();
        m.insert("name".into(), json!(name));
        m.insert("profile".into(), json!(cur.profile));
        m.insert("focal_length_mm".into(), json!(cur.optics.focal_length_mm));
        m.insert("focal_multiplier".into(), json!(cur.optics.focal_multiplier));
        m.insert("pixel_size_um".into(), json!(cur.optics.pixel_size_um));
        m.insert("binning".into(), json!(cur.optics.binning));
        m.insert("flat".into(), json!(cur.flat));
        m.insert("dark".into(), json!(cur.dark));
        m.insert("dark_exposure_us".into(), json!(cur.dark_exposure_us));
        m.insert("dark_gain".into(), json!(cur.dark_gain));
        for (k, v) in &obj {
            if matches!(k.as_str(), "profile" | "focal_length_mm" | "focal_multiplier" | "pixel_size_um" | "binning" | "aperture_mm" | "notes" | "flat" | "dark" | "dark_exposure_us" | "dark_gain") {
                m.insert(k.clone(), v.clone());
            }
        }
        let activate = obj.get("activate").and_then(|v| v.as_bool());
        let s: OpticalSetup = serde_json::from_value(Value::Object(m)).map_err(|e| format!("not a valid setup: {e}"))?;
        let s = s.validate()?;
        match self.list.iter_mut().find(|x| x.name == s.name) {
            Some(x) => *x = s.clone(),
            None => {
                if self.list.len() >= 50 {
                    return Err("at most 50 optical setups: delete one first".into());
                }
                self.list.push(s.clone());
            }
        }
        if activate.unwrap_or(true) && (self.active.as_deref() == Some(&s.name) || !s.differs_from(cur)) {
            self.active = Some(s.name.clone());
        }
        Ok(s)
    }

    /// Removes a setup; if it was the active one nothing is active afterwards (the optics in use stay as they are).
    pub fn delete(&mut self, name: &str) -> Result<OpticalSetup, String> {
        let n = name.trim();
        let i = self.list.iter().position(|s| s.name == n).ok_or_else(|| format!("{NOT_FOUND} {n}"))?;
        let gone = self.list.remove(i);
        if self.active.as_deref() == Some(n) {
            self.active = None;
        }
        Ok(gone)
    }

    pub fn to_json(&self, cur: &Current) -> Value {
        json!({ "active": self.active, "modified": self.modified(cur), "setups": self.list })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cur() -> Current {
        Current { optics: OpticsSettings { focal_length_mm: Some(400.0), focal_multiplier: 2.0, pixel_size_um: None, binning: 1 }, profile: "sun_ha".into(), flat: Some("flats/f.png".into()), dark: None, dark_exposure_us: None, dark_gain: None }
    }
    fn tmp(tag: &str) -> std::path::PathBuf {
        let f = std::env::temp_dir().join(format!("rustflare-setups-{tag}-{}.json", std::process::id()));
        std::fs::remove_file(&f).ok();
        f
    }

    #[test]
    fn names_are_trimmed_and_checked() {
        assert_eq!(clean_name("  Lunt + Barlow ").unwrap(), "Lunt + Barlow");
        assert!(clean_name("   ").is_err());
        assert!(clean_name(&"x".repeat(41)).is_err());
        assert!(clean_name(&"x".repeat(40)).is_ok());
        assert!(clean_name("a\nb").is_err());
        assert!(clean_name("a\u{7}b").is_err());
        assert!(clean_name("a/b").is_err());
    }

    #[test]
    fn save_from_current_fills_everything_and_validates() {
        let mut s = Setups::default();
        let a = s.save(" Lunt ", &json!({}), &cur()).unwrap();
        assert_eq!(a.name, "Lunt");
        assert_eq!(a.profile.as_deref(), Some("sun_ha"));
        assert_eq!(a.focal_length_mm, Some(400.0));
        assert_eq!(a.focal_multiplier, 2.0);
        assert_eq!(a.flat.as_deref(), Some("flats/f.png"));
        assert_eq!(s.active.as_deref(), Some("Lunt"));
        assert!(!s.modified(&cur()));
        // bad input is refused and nothing is stored
        assert!(s.save("B", &json!({"profile": "sun"}), &cur()).is_err());
        assert!(s.save("B", &json!({"focal_length_mm": 5}), &cur()).is_err());
        assert!(s.save("B", &json!({"focal_multiplier": 11}), &cur()).is_err());
        assert!(s.save("B", &json!({"binning": 0}), &cur()).is_err());
        assert!(s.save("B", &json!({"aperture_mm": 1}), &cur()).is_err());
        assert!(s.save("B", &json!({"notes": "x".repeat(501)}), &cur()).is_err());
        assert_eq!(s.list.len(), 1);
    }

    #[test]
    fn names_are_unique_and_replacing_keeps_the_order() {
        let mut s = Setups::default();
        s.save("A", &json!({}), &cur()).unwrap();
        s.save("B", &json!({"profile": "moon"}), &cur()).unwrap();
        s.save("A", &json!({"notes": "  new  ", "aperture_mm": 50}), &cur()).unwrap();
        assert_eq!(s.list.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["A", "B"]);
        assert_eq!(s.list[0].notes.as_deref(), Some("new"));
        assert_eq!(s.list[0].aperture_mm, Some(50.0));
        // a blank note is none
        s.save("A", &json!({"notes": "   "}), &cur()).unwrap();
        assert_eq!(s.list[0].notes, None);
        // the same name with other spacing is the same name
        s.save("  B ", &json!({}), &cur()).unwrap();
        assert_eq!(s.list.len(), 2);
    }

    #[test]
    fn modified_follows_optics_and_profile() {
        let mut s = Setups::default();
        s.save("A", &json!({}), &cur()).unwrap();
        let mut c = cur();
        assert!(!s.modified(&c));
        c.optics.focal_multiplier = 1.0;
        assert!(s.modified(&c));
        let mut c = cur();
        c.profile = "moon".into();
        assert!(s.modified(&c), "the profile counts");
        let mut c = cur();
        c.optics.pixel_size_um = Some(2.9);
        assert!(s.modified(&c));
        let mut c = cur();
        c.optics.binning = 2;
        assert!(s.modified(&c));
        // a setup without a profile leaves the profile out of the comparison
        s.save("A", &json!({"profile": null}), &cur()).unwrap();
        let mut c = cur();
        c.profile = "moon".into();
        assert!(!s.modified(&c));
        // flats and darks do not count
        let mut c = cur();
        c.flat = None;
        assert!(!s.modified(&c));
        // nothing active: never modified
        s.active = None;
        c.optics.binning = 3;
        assert!(!s.modified(&c));
    }

    #[test]
    fn saving_a_modified_state_under_another_name_does_not_move_the_active_setup() {
        let mut s = Setups::default();
        s.save("A", &json!({}), &cur()).unwrap();
        // explicit body that differs from what is in use: stored, but A stays active
        s.save("B", &json!({"focal_length_mm": 1000}), &cur()).unwrap();
        assert_eq!(s.active.as_deref(), Some("A"));
        // a duplicate that asks not to be activated
        s.save("C", &json!({"activate": false}), &cur()).unwrap();
        assert_eq!(s.active.as_deref(), Some("A"));
        // replacing the active one keeps it active (and then shows modified if it differs)
        s.save("A", &json!({"focal_length_mm": 900}), &cur()).unwrap();
        assert_eq!(s.active.as_deref(), Some("A"));
        assert!(s.modified(&cur()));
    }

    #[test]
    fn delete_removes_and_clears_the_active_name() {
        let mut s = Setups::default();
        s.save("A", &json!({}), &cur()).unwrap();
        s.save("B", &json!({"focal_length_mm": 1000}), &cur()).unwrap();
        assert!(s.delete("nope").unwrap_err().starts_with(NOT_FOUND));
        s.delete("B").unwrap();
        assert_eq!(s.active.as_deref(), Some("A"));
        s.delete(" A ").unwrap();
        assert!(s.list.is_empty());
        assert_eq!(s.active, None);
    }

    #[test]
    fn survives_a_restart_through_the_settings_file() {
        let f = tmp("persist");
        let mut s = Setups::load(&f);
        assert!(s.list.is_empty() && s.active.is_none(), "nothing is created by itself");
        s.save("Lunt + Barlow", &json!({"aperture_mm": 50, "notes": "H-alpha"}), &cur()).unwrap();
        s.save("C11", &json!({"profile": "moon", "focal_length_mm": 2800, "focal_multiplier": 0.63}), &cur()).unwrap();
        s.persist(&f).unwrap();
        // other keys of the file are not disturbed
        crate::config::set(&f, "profile", json!("sun_ha")).unwrap();
        let r = Setups::load(&f);
        assert_eq!(r.list, s.list);
        assert_eq!(r.active.as_deref(), Some("Lunt + Barlow"));
        assert_eq!(crate::config::read(&f)["profile"], json!("sun_ha"));
        // an active name that is not in the list is dropped; a damaged entry is skipped
        crate::config::set(&f, "optical_setup", json!("ghost")).unwrap();
        crate::config::set(&f, "optical_setups", json!([{"name": "ok", "focal_length_mm": 500.0}, {"name": "", "focal_length_mm": 1.0}, 7])).unwrap();
        let r = Setups::load(&f);
        assert_eq!(r.list.len(), 1);
        assert_eq!(r.list[0].binning, 1);
        assert_eq!(r.active, None);
        std::fs::remove_file(&f).ok();
    }

    /// The engine end to end on the simulators: save from the current optics, change, see "modified", apply (the profile follows, a missing
    /// flat is reported and does not fail it), delete.
    #[test]
    fn the_engine_saves_applies_and_deletes_setups() {
        use crate::engine::Command;
        let dir = std::env::temp_dir().join(format!("rustflare-setups-engine-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        let storage = std::sync::Arc::new(crate::storage::Storage::load(dir.join("settings.json"), dir.join("data")));
        let (h, _t) = crate::engine::spawn(storage, Default::default(), Default::default());
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let call = |c: Command| rt.block_on(h.call(c));
        let optics = |f: f64, m: f64| Command::SetOptics { focal_length_mm: Some(f), focal_multiplier: m, pixel_size_um: Some(3.76), binning: 1 };
        let list = || call(Command::ListSetups).unwrap();
        assert_eq!(list()["setups"], json!([]), "nothing is created by itself");
        call(optics(400.0, 2.0)).unwrap();
        call(Command::SetProfile { id: "sun_ha".into() }).unwrap();
        let r = call(Command::SaveSetup { name: "Lunt + Barlow".into(), body: json!({"aperture_mm": 50, "flat": "flats/gone.png"}) }).unwrap();
        assert_eq!(r["active"], json!("Lunt + Barlow"));
        call(Command::SaveSetup { name: "C11".into(), body: json!({"profile": "moon", "focal_length_mm": 2800, "focal_multiplier": 0.63}) }).unwrap();
        assert_eq!(list()["active"], json!("Lunt + Barlow"));
        // changing the optics afterwards shows as modified
        call(optics(500.0, 2.0)).unwrap();
        assert_eq!(list()["modified"], json!(true));
        assert!(call(Command::SaveSetup { name: "x".into(), body: json!({"profile": "sun"}) }).is_err());
        // apply the other train: its profile follows, nothing is created, the settings file has it
        let r = call(Command::ApplySetup { name: "C11".into() }).unwrap();
        assert_eq!(r["after"]["profile"], json!("moon"));
        assert_eq!(r["after"]["focal_length_mm"], json!(2800.0));
        assert_eq!(r["before"]["profile"], json!("sun_ha"));
        let l = list();
        assert_eq!((l["active"].clone(), l["modified"].clone()), (json!("C11"), json!(false)));
        assert_eq!(crate::config::read(&dir.join("settings.json"))["profile"], json!("moon"));
        // back: the flat is not there, which is reported, and the rest is applied
        let r = call(Command::ApplySetup { name: "Lunt + Barlow".into() }).unwrap();
        assert_eq!(r["after"]["profile"], json!("sun_ha"));
        assert_eq!(r["not_restored"][0]["what"], json!("flat"));
        assert!(call(Command::ApplySetup { name: "nope".into() }).unwrap_err().starts_with(NOT_FOUND));
        call(Command::DeleteSetup { name: "Lunt + Barlow".into() }).unwrap();
        assert_eq!(list()["active"], json!(null));
        assert!(call(Command::DeleteSetup { name: "Lunt + Barlow".into() }).unwrap_err().starts_with(NOT_FOUND));
        // a restart finds what is left
        assert_eq!(Setups::load(&dir.join("settings.json")).list.len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_setup_remembers_the_exposure_and_gain_of_its_dark() {
        let mut c = cur();
        c.dark = Some("darks/d.png".into());
        c.dark_exposure_us = Some(2500.0);
        c.dark_gain = Some(120.0);
        let mut l = Setups::default();
        let s = l.save("Lunt + Barlow", &json!({}), &c).unwrap();
        assert_eq!((s.dark.as_deref(), s.dark_exposure_us, s.dark_gain), (Some("darks/d.png"), Some(2500.0), Some(120.0)));
        // an older saved setup without the fields still loads
        let old: OpticalSetup = serde_json::from_value(json!({ "name": "old", "focal_length_mm": 500.0, "dark": "darks/x.png" })).unwrap();
        assert_eq!((old.dark_exposure_us, old.dark_gain), (None, None));
    }
}
