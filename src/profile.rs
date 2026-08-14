//! Saved settings, stored as JSON under ~/.config/kiyoctl/profiles/.

use crate::controls::{self, Kind};
use crate::usb::Cam;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Profile {
    /// The camera this profile was captured from, as "vvvv:pppp".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub controls: BTreeMap<String, Json>,
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
        serde_json::from_str(&text).map_err(|e| format!("profile '{name}' is not valid JSON: {e}"))
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
    pub fn set(&mut self, control: &str, value: &str) {
        let json = match controls::find_any(control).map(|c| &c.kind) {
            Some(Kind::Int { .. }) => value
                .parse::<i64>()
                .map(Json::from)
                .unwrap_or_else(|_| Json::from(value)),
            _ => Json::from(value),
        };
        self.controls.insert(control.to_string(), json);
    }

    /// The stored value rendered the way the CLI accepts it.
    pub fn get(&self, control: &str) -> Option<String> {
        self.controls.get(control).map(render)
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

fn render(v: &Json) -> String {
    match v {
        Json::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Snapshot the camera's readable controls into `profile`, leaving write-only
/// extension-unit entries as they were (the camera cannot report those).
pub fn capture(cam: &Cam, profile: &mut Profile) -> Vec<String> {
    let mut captured = Vec::new();
    for ctrl in controls::STANDARD {
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
    profile.device = Some(format!("{:04x}:{:04x}", cam.vid, cam.pid));
    profile.name = Some(cam.name.clone());
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

    let mut planned: Vec<(&'static crate::controls::Control, String)> = profile
        .controls
        .iter()
        .filter_map(|(name, value)| controls::find_any(name).map(|c| (c, render(value))))
        .collect();
    planned.sort_by_key(|(c, _)| c.order);

    // Unknown names are worth surfacing rather than silently dropping.
    for name in profile.controls.keys() {
        if controls::find_any(name).is_none() {
            report.skipped.push((name.clone(), "unknown control".into()));
        }
    }

    let mut touched_extension = false;
    for (ctrl, value) in &planned {
        if ctrl.is_opaque() && !cam.has_razer_unit() {
            report
                .skipped
                .push((ctrl.name.into(), "camera has no Razer extension unit".into()));
            continue;
        }

        // Honour prerequisites using the values this profile is establishing,
        // falling back to what the camera currently reports.
        if let Some((dep, allowed)) = ctrl.requires {
            let dep_value = profile
                .get(dep)
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
