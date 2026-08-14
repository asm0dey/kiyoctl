//! Saved settings, stored as JSON under ~/.config/kiyoctl/profiles/.

use crate::controls::{self, Kind};
use crate::models;
use crate::usb::Cam;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Profile {
    /// The camera this profile was last captured from, as "vvvv:pppp".
    /// Read from `device` too, which is what 0.2.0 wrote.
    #[serde(skip_serializing_if = "Option::is_none", alias = "device")]
    pub camera: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Standard UVC control values. Portable to any camera.
    pub controls: BTreeMap<String, Json>,
    /// Model control values, keyed by the camera they belong to. These cannot
    /// be read back from hardware, so they are never dropped. See ADR 0003.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub per_camera: BTreeMap<String, BTreeMap<String, Json>>,
}

pub fn config_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    Path::new(&home).join(".config").join("kiyoctl")
}

pub fn profile_path(name: &str) -> PathBuf {
    config_dir().join("profiles").join(format!("{name}.json"))
}

/// The profile used when none is named on the command line. Picking a profile
/// in the UI (or `kiyoctl use`) records it here, so the choice outlives the
/// process that made it.
pub fn active_path() -> PathBuf {
    config_dir().join("active")
}

pub const DEFAULT: &str = "default";

pub fn active() -> String {
    let recorded = std::fs::read_to_string(active_path())
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    // A profile deleted from under us should not wedge every later command.
    match recorded {
        Some(name) if profile_path(&name).exists() => name,
        _ => DEFAULT.to_string(),
    }
}

pub fn set_active(name: &str) -> Result<(), String> {
    let path = active_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    std::fs::write(&path, format!("{name}\n"))
        .map_err(|e| format!("cannot record the active profile in {}: {e}", path.display()))
}

impl Profile {
    pub fn load(name: &str) -> Result<Profile, String> {
        let path = profile_path(name);
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read profile '{name}' ({}): {e}", path.display()))?;
        Profile::from_json(&text).map_err(|e| format!("profile '{name}' {e}"))
    }

    /// Parse a stored profile. Every read goes through here so nothing can
    /// reach the rest of kiyoctl unmigrated.
    pub fn from_json(text: &str) -> Result<Profile, String> {
        let mut profile: Profile =
            serde_json::from_str(text).map_err(|e| format!("is not valid JSON: {e}"))?;
        profile.migrate();
        Ok(profile)
    }

    /// The section key for a camera.
    pub fn key(vid: u16, pid: u16) -> String {
        format!("{vid:04x}:{pid:04x}")
    }

    /// Say which camera this profile is being edited against. Do this before
    /// any `set`, which uses the identity to route a Model control into the
    /// right section. Values recorded for another camera stay in theirs.
    pub fn home_to(&mut self, cam: &Cam) {
        self.camera = Some(Profile::key(cam.vid, cam.pid));
        self.name = Some(cam.name.clone());
    }

    /// Move Model control values out of the flat map and into the section for
    /// the camera this profile records.
    ///
    /// Profiles written by 0.2.0 keep `hdr` and `fov` in the flat map. Applying
    /// that map as standard-controls-only would silently stop applying them, so
    /// this runs on every load. Idempotent.
    ///
    /// A value only moves when the recorded camera's own Model declares it.
    /// Anything else stays flat, where it goes on applying to whatever camera
    /// declares it: 0.2.0's `set` overwrote the recorded identity on every
    /// camera it touched, so a profile can name camera B while carrying camera
    /// A's vendor values, and these values cannot be read back from hardware —
    /// never relocate one on a guess. Same reason a profile with no recorded
    /// camera keeps everything flat. See ADR 0003.
    pub fn migrate(&mut self) {
        let Some(camera) = self.camera.clone() else { return };
        let Some(model) = parse_key(&camera).and_then(|(vid, pid)| models::for_camera(vid, pid))
        else {
            return;
        };
        let moving: Vec<String> = self
            .controls
            .keys()
            .filter(|name| model.controls.iter().any(|c| c.name == name.as_str()))
            .cloned()
            .collect();
        if moving.is_empty() {
            return;
        }
        let section = self.per_camera.entry(camera).or_default();
        for name in moving {
            if let Some(value) = self.controls.remove(&name) {
                section.entry(name).or_insert(value);
            }
        }
    }

    /// Every value that should be written to this camera: the portable ones,
    /// plus its own section. A value in the flat map that belongs to a Model
    /// is still applied — that is the un-migratable case from `migrate`.
    pub fn values_for(&self, vid: u16, pid: u16) -> BTreeMap<String, Json> {
        let mut out = self.controls.clone();
        if let Some(section) = self.per_camera.get(&Profile::key(vid, pid)) {
            for (k, v) in section {
                out.insert(k.clone(), v.clone());
            }
        }
        out
    }

    /// Load a profile, or return an empty one if it does not exist yet.
    pub fn load_or_default(name: &str) -> Result<Profile, String> {
        if profile_path(name).exists() {
            Profile::load(name)
        } else {
            Ok(Profile::default())
        }
    }

    pub fn save(&self, name: &str) -> Result<PathBuf, String> {
        let path = profile_path(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| format!("cannot serialise profile: {e}"))?;
        std::fs::write(&path, text + "\n")
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        Ok(path)
    }

    /// Record one control value, preserving natural JSON types.
    ///
    /// `camera` must be set first for a Model control to reach its section.
    pub fn set(&mut self, control: &str, value: &str) {
        let json = match controls::find_any(control).map(|c| &c.kind) {
            Some(Kind::Int { .. }) => value
                .parse::<i64>()
                .map(Json::from)
                .unwrap_or_else(|_| Json::from(value)),
            _ => Json::from(value),
        };
        match (controls::is_model_control(control), self.camera.clone()) {
            (true, Some(camera)) => {
                self.per_camera
                    .entry(camera)
                    .or_default()
                    .insert(control.to_string(), json);
            }
            _ => {
                self.controls.insert(control.to_string(), json);
            }
        }
    }

    /// The stored value rendered the way the CLI accepts it. Searches the flat
    /// map, then the section of the camera this profile records.
    pub fn get(&self, control: &str) -> Option<String> {
        if let Some(v) = self.controls.get(control) {
            return Some(render(v));
        }
        let mine = self.camera.as_ref().and_then(|c| self.per_camera.get(c));
        mine.and_then(|s| s.get(control)).map(render)
    }

    /// The camera this profile was captured from, when that is not the camera
    /// being applied to. Informational: cross-camera profiles are legitimate,
    /// since standard UVC values transfer.
    pub fn foreign_to(&self, vid: u16, pid: u16) -> Option<String> {
        let recorded = self.camera.clone()?;
        (recorded != Profile::key(vid, pid)).then_some(recorded)
    }

    pub fn list() -> Vec<String> {
        let dir = config_dir().join("profiles");
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let p = e.path();
                if p.extension().and_then(|s| s.to_str()) == Some("json") {
                    p.file_stem().and_then(|s| s.to_str()).map(str::to_string)
                } else {
                    None
                }
            })
            .collect();
        names.sort();
        names
    }
}

/// The inverse of `Profile::key`. Anything else recorded there is not a camera
/// this kiyoctl can reason about.
fn parse_key(key: &str) -> Option<(u16, u16)> {
    let (vid, pid) = key.split_once(':')?;
    Some((u16::from_str_radix(vid, 16).ok()?, u16::from_str_radix(pid, 16).ok()?))
}

pub(crate) fn render(v: &Json) -> String {
    match v {
        Json::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Snapshot the camera's readable controls into `profile`, leaving write-only
/// extension-unit entries as they were (the camera cannot report those).
pub fn capture(cam: &Cam, profile: &mut Profile) -> Vec<String> {
    // Identity first: `set` uses it to decide which map a Model control
    // belongs in. Any previous camera's values are already in their own
    // section, so re-homing loses nothing.
    profile.home_to(cam);

    let mut captured = Vec::new();
    for ctrl in cam.controls() {
        if ctrl.is_opaque() {
            continue;
        }
        if let Ok(Some(reading)) = controls::read(cam, ctrl) {
            if reading.writable {
                profile.set(ctrl.name, &reading.value);
                captured.push(ctrl.name.to_string());
            }
        }
    }
    captured
}

pub struct ApplyReport {
    pub applied: Vec<String>,
    pub skipped: Vec<(String, String)>,
}

/// Write every control in the profile back to the camera.
///
/// Controls are ordered so that the ones that gate others (auto white balance,
/// exposure mode) land first, and a control whose prerequisite is not satisfied
/// is skipped rather than reported as a failure.
pub fn apply(cam: &Cam, profile: &Profile) -> ApplyReport {
    let mut report = ApplyReport { applied: Vec::new(), skipped: Vec::new() };

    let values = profile.values_for(cam.vid, cam.pid);

    let mut planned: Vec<(&'static crate::controls::Control, String)> = values
        .iter()
        .filter_map(|(name, value)| cam.find(name).map(|c| (c, render(value))))
        .collect();
    planned.sort_by_key(|(c, _)| c.order);

    // A name kiyoctl knows but this camera lacks reads very differently from a
    // name nobody has heard of.
    for name in values.keys() {
        if cam.find(name).is_none() {
            let why = if controls::find_any(name).is_some() {
                "not available on this camera"
            } else {
                "unknown control"
            };
            report.skipped.push((name.clone(), why.into()));
        }
    }

    let mut touched_extension = false;
    for (ctrl, value) in &planned {
        // Honour prerequisites using the values this profile is establishing,
        // falling back to what the camera currently reports.
        if let Some((dep, allowed)) = ctrl.requires {
            let dep_value = values
                .get(dep)
                .map(render)
                .or_else(|| controls::find_any(dep).and_then(|d| controls::read(cam, d).ok().flatten()).map(|r| r.value));
            match dep_value {
                Some(v) if allowed.contains(&v.as_str()) => {}
                Some(v) => {
                    report
                        .skipped
                        .push((ctrl.name.into(), format!("requires {dep}={}, but it is {v}", allowed.join(" or "))));
                    continue;
                }
                None => {}
            }
        }

        match controls::write(cam, ctrl, value) {
            Ok(()) => {
                report.applied.push(format!("{} = {}", ctrl.name, value));
                if matches!(ctrl.unit, crate::usb::Unit::Extension(_)) {
                    touched_extension = true;
                }
            }
            Err(e) => report.skipped.push((ctrl.name.into(), e)),
        }
    }

    // Persist extension-unit state into the camera's own storage so it survives
    // a power cycle even without kiyoctl running.
    if touched_extension {
        cam.persist();
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exactly what kiyoctl 0.2.0 wrote: a flat map with hdr and fov in it.
    const OLD_FORMAT: &str = r#"{
        "device": "1532:0e05",
        "name": "Razer Kiyo Pro",
        "controls": { "brightness": 129, "hdr": "on", "fov": "wide" }
    }"#;

    #[test]
    fn an_old_profile_still_names_its_camera() {
        let p: Profile = serde_json::from_str(OLD_FORMAT).unwrap();
        assert_eq!(p.camera.as_deref(), Some("1532:0e05"), "the device alias must be read");
    }

    #[test]
    fn migration_moves_model_controls_into_their_camera_section() {
        let mut p: Profile = serde_json::from_str(OLD_FORMAT).unwrap();
        p.migrate();
        assert!(!p.controls.contains_key("hdr"), "hdr must leave the flat map");
        assert!(p.controls.contains_key("brightness"), "standard controls stay flat");
        let section = p.per_camera.get("1532:0e05").expect("a section for the Kiyo Pro");
        assert_eq!(section.get("hdr").unwrap(), "on");
        assert_eq!(section.get("fov").unwrap(), "wide");
    }

    /// The regression guard for ADR 0003: an upgraded profile must produce the
    /// same set of values to apply as 0.2.0 did.
    #[test]
    fn migration_does_not_change_what_gets_applied() {
        let mut p: Profile = serde_json::from_str(OLD_FORMAT).unwrap();
        p.migrate();
        let values = p.values_for(0x1532, 0x0e05);
        assert_eq!(values.get("brightness").unwrap(), 129);
        assert_eq!(values.get("hdr").unwrap(), "on");
        assert_eq!(values.get("fov").unwrap(), "wide");
        assert_eq!(values.len(), 3, "nothing gained, nothing lost");
    }

    #[test]
    fn a_profile_with_no_recorded_camera_keeps_its_values_flat() {
        let mut p: Profile = serde_json::from_str(
            r#"{ "controls": { "brightness": 129, "hdr": "on" } }"#,
        )
        .unwrap();
        p.migrate();
        assert!(
            p.controls.contains_key("hdr"),
            "with no camera recorded, guessing which section to use is worse than leaving it"
        );
        assert!(p.per_camera.is_empty());
        // And it must still be applied, to whatever camera declares it.
        assert_eq!(p.values_for(0x1532, 0x0e05).get("hdr").unwrap(), "on");
    }

    #[test]
    fn another_cameras_section_is_not_applied() {
        let mut p = Profile::default();
        p.controls.insert("brightness".into(), Json::from(129));
        p.per_camera.insert(
            "1532:0e05".into(),
            [("hdr".to_string(), Json::from("on"))].into_iter().collect(),
        );
        let values = p.values_for(0x046d, 0x085e);
        assert!(values.contains_key("brightness"), "standard controls are portable");
        assert!(!values.contains_key("hdr"), "another camera's section must not apply");
    }

    /// 0.2.0's `set` overwrote the recorded camera on every camera it touched,
    /// so a profile can name camera B while carrying camera A's hdr. Homing
    /// that value to B would stop it ever applying to A again, and it cannot be
    /// read back off A to recover it.
    #[test]
    fn a_value_the_recorded_camera_does_not_have_is_left_where_it_still_applies() {
        let mut p: Profile = serde_json::from_str(
            r#"{
                "device": "046d:085e",
                "controls": { "brightness": 129, "hdr": "on" }
            }"#,
        )
        .unwrap();
        p.migrate();
        assert!(p.controls.contains_key("hdr"), "the Logitech has no hdr to home it to");
        assert!(p.per_camera.is_empty());
        assert_eq!(p.values_for(0x1532, 0x0e05).get("hdr").unwrap(), "on", "still reaches the Kiyo Pro");
    }

    /// Two copies of one name: the section is what `values_for` would have
    /// applied, so migration must not let the flat one overwrite it.
    #[test]
    fn migration_keeps_the_section_value_when_both_maps_hold_a_name() {
        let mut p: Profile = serde_json::from_str(
            r#"{
                "camera": "1532:0e05",
                "controls": { "hdr": "off" },
                "per_camera": { "1532:0e05": { "hdr": "on" } }
            }"#,
        )
        .unwrap();
        p.migrate();
        assert!(!p.controls.contains_key("hdr"), "the flat duplicate goes");
        assert_eq!(p.values_for(0x1532, 0x0e05)["hdr"], "on", "the section wins, as it does in values_for");
    }

    #[test]
    fn reading_a_profile_migrates_it() {
        // `load` only reads the file; `from_json` is the whole of the rest.
        let p = Profile::from_json(OLD_FORMAT).unwrap();
        assert_eq!(p.per_camera["1532:0e05"]["hdr"], "on", "parsing must migrate, not just deserialise");
        assert!(!p.controls.contains_key("hdr"));
    }

    #[test]
    fn migration_is_idempotent() {
        let mut p: Profile = serde_json::from_str(OLD_FORMAT).unwrap();
        p.migrate();
        let once = p.values_for(0x1532, 0x0e05);
        p.migrate();
        assert_eq!(p.values_for(0x1532, 0x0e05), once);
    }

    #[test]
    fn set_routes_a_model_control_into_the_current_cameras_section() {
        let mut p = Profile::default();
        p.camera = Some("1532:0e05".into());
        p.set("hdr", "on");
        p.set("brightness", "129");
        assert!(!p.controls.contains_key("hdr"), "a Model control does not go in the flat map");
        assert_eq!(p.per_camera["1532:0e05"]["hdr"], "on");
        assert_eq!(p.controls["brightness"], 129, "Int controls keep their JSON type");
    }

    /// The chimera bug: capturing from a second camera used to overwrite the
    /// profile's identity while keeping the first camera's opaque values, so
    /// they applied to the wrong hardware. Sections make that impossible.
    #[test]
    fn re_homing_a_profile_does_not_strand_the_first_cameras_values() {
        let mut p = Profile::default();
        p.camera = Some("1532:0e05".into());
        p.set("hdr", "on");
        // Now capture from a different camera, as `capture` does: identity first.
        p.camera = Some("046d:085e".into());
        p.set("brightness", "140");
        assert_eq!(
            p.per_camera["1532:0e05"]["hdr"], "on",
            "the first camera's value must survive"
        );
        assert!(!p.values_for(0x046d, 0x085e).contains_key("hdr"));
        assert_eq!(p.values_for(0x1532, 0x0e05)["hdr"], "on");
    }

    #[test]
    fn a_profile_from_another_camera_is_flagged() {
        let mut p = Profile::default();
        p.camera = Some("1532:0e05".into());
        assert_eq!(p.foreign_to(0x046d, 0x085e).as_deref(), Some("1532:0e05"));
        assert_eq!(p.foreign_to(0x1532, 0x0e05), None, "its own camera is not foreign");
    }

    #[test]
    fn a_profile_with_no_recorded_camera_is_never_foreign() {
        assert_eq!(Profile::default().foreign_to(0x046d, 0x085e), None);
    }

    #[test]
    fn a_new_profile_round_trips_through_json() {
        let mut p = Profile::default();
        p.camera = Some("1532:0e05".into());
        p.controls.insert("brightness".into(), Json::from(129));
        p.per_camera.insert(
            "1532:0e05".into(),
            [("hdr".to_string(), Json::from("on"))].into_iter().collect(),
        );
        let text = serde_json::to_string(&p).unwrap();
        let back: Profile = serde_json::from_str(&text).unwrap();
        assert_eq!(back.values_for(0x1532, 0x0e05), p.values_for(0x1532, 0x0e05));
        assert!(text.contains("\"camera\""), "new profiles write the new key");
    }
}
