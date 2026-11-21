//! Tiny JSON settings file shared by features (storage location, optics). Each feature owns one key;
//! writes merge into the existing file and are atomic (write temp + rename).

use serde_json::{Map, Value};
use std::path::Path;

pub fn read(file: &Path) -> Map<String, Value> {
    std::fs::read(file)
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

pub fn set(file: &Path, key: &str, value: Value) -> Result<(), String> {
    let mut m = read(file);
    m.insert(key.to_string(), value);
    let tmp = file.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&Value::Object(m)).unwrap_or_default())
        .and_then(|_| std::fs::rename(&tmp, file))
        .map_err(|e| format!("cannot save settings to {}: {e}", file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keys_do_not_clobber_each_other() {
        let f = std::env::temp_dir().join(format!("rustflare-config-test-{}.json", std::process::id()));
        std::fs::remove_file(&f).ok();
        set(&f, "data_dir", json!("/x")).unwrap();
        set(&f, "optics", json!({"focal_length_mm": 700})).unwrap();
        let m = read(&f);
        assert_eq!(m["data_dir"], json!("/x"));
        assert_eq!(m["optics"]["focal_length_mm"], json!(700));
        std::fs::remove_file(&f).ok();
    }
}
