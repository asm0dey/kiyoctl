//! The catalogue of controls kiyoctl knows how to read and write.
//!
//! Standard UVC controls live on the camera terminal or processing unit and are
//! readable. A Model's extension-unit controls are write-only — the camera
//! accepts them but never reports them back — so their state is tracked in the
//! saved profile rather than queried from hardware. They live in `src/models/`.

use crate::models::{Model, MODELS};
use crate::usb::{Cam, Unit, GET_CUR, GET_DEF, GET_MAX, GET_MIN, GET_RES, INFO_GET, INFO_SET};

/// A single named payload of an opaque control.
pub struct OpaqueOpt {
    pub name: &'static str,
    /// Payload written to the control's selector.
    pub payload: &'static [u8],
    /// Some options need a priming write first (the field-of-view ones do).
    pub pre: Option<&'static [u8]>,
}

pub enum Kind {
    Int { signed: bool },
    Bool,
    /// Named values over a small integer domain.
    Menu(&'static [(&'static str, i64)]),
    /// Write-only named byte payloads, with no read-back. Independent of which
    /// unit the control lives on — see CONTEXT.md.
    Opaque(&'static [OpaqueOpt]),
}

pub struct Control {
    pub name: &'static str,
    pub unit: Unit,
    pub selector: u8,
    pub len: usize,
    pub kind: Kind,
    pub help: &'static str,
    /// Controls that gate others are applied first.
    pub order: u8,
    /// This control is only writable while `.0` holds one of the values `.1`.
    pub requires: Option<(&'static str, &'static [&'static str])>,
}

const POWER_LINE: &[(&str, i64)] =
    &[("disabled", 0), ("50hz", 1), ("60hz", 2), ("auto", 3)];

// UVC encodes the exposure mode as a bitmap, one bit per mode.
const AE_MODE: &[(&str, i64)] = &[
    ("manual", 1),
    ("auto", 2),
    ("shutter_priority", 4),
    ("aperture_priority", 8),
];

pub static STANDARD: &[Control] = &[
    // --- Processing unit -------------------------------------------------
    Control { name: "brightness", unit: Unit::Processing, selector: 0x02, len: 2,
        kind: Kind::Int { signed: true }, help: "image brightness", order: 1, requires: None },
    Control { name: "contrast", unit: Unit::Processing, selector: 0x03, len: 2,
        kind: Kind::Int { signed: false }, help: "image contrast", order: 1, requires: None },
    Control { name: "saturation", unit: Unit::Processing, selector: 0x07, len: 2,
        kind: Kind::Int { signed: false }, help: "colour saturation", order: 1, requires: None },
    Control { name: "sharpness", unit: Unit::Processing, selector: 0x08, len: 2,
        kind: Kind::Int { signed: false }, help: "edge sharpening", order: 1, requires: None },
    Control { name: "gamma", unit: Unit::Processing, selector: 0x09, len: 2,
        kind: Kind::Int { signed: false }, help: "gamma correction", order: 1, requires: None },
    Control { name: "gain", unit: Unit::Processing, selector: 0x04, len: 2,
        kind: Kind::Int { signed: false }, help: "sensor gain", order: 1, requires: None },
    Control { name: "hue", unit: Unit::Processing, selector: 0x06, len: 2,
        kind: Kind::Int { signed: true }, help: "colour hue", order: 1, requires: None },
    Control { name: "backlight_compensation", unit: Unit::Processing, selector: 0x01, len: 2,
        kind: Kind::Int { signed: false }, help: "backlight compensation", order: 1, requires: None },
    Control { name: "power_line_frequency", unit: Unit::Processing, selector: 0x05, len: 1,
        kind: Kind::Menu(POWER_LINE), help: "anti-flicker mains frequency", order: 1, requires: None },
    Control { name: "white_balance_auto", unit: Unit::Processing, selector: 0x0b, len: 1,
        kind: Kind::Bool, help: "automatic white balance", order: 0, requires: None },
    Control { name: "white_balance", unit: Unit::Processing, selector: 0x0a, len: 2,
        kind: Kind::Int { signed: false }, help: "white balance temperature (K)", order: 1,
        requires: Some(("white_balance_auto", &["off"])) },

    // --- Camera terminal -------------------------------------------------
    Control { name: "auto_exposure", unit: Unit::Camera, selector: 0x02, len: 1,
        kind: Kind::Menu(AE_MODE), help: "exposure mode", order: 0, requires: None },
    Control { name: "exposure_time", unit: Unit::Camera, selector: 0x04, len: 4,
        kind: Kind::Int { signed: false }, help: "exposure time (100 us units)", order: 1,
        requires: Some(("auto_exposure", &["manual", "shutter_priority"])) },
    Control { name: "focus_auto", unit: Unit::Camera, selector: 0x08, len: 1,
        kind: Kind::Bool, help: "automatic focus", order: 0, requires: None },
    Control { name: "focus", unit: Unit::Camera, selector: 0x06, len: 2,
        kind: Kind::Int { signed: false }, help: "focus position", order: 1,
        requires: Some(("focus_auto", &["off"])) },
    Control { name: "zoom", unit: Unit::Camera, selector: 0x0b, len: 2,
        kind: Kind::Int { signed: false }, help: "optical zoom", order: 1, requires: None },
];

/// The controls a camera covered by `model` has: the standard UVC catalogue
/// plus whatever that Model adds. Pure, so it is testable without hardware.
pub fn effective_controls(model: Option<&'static Model>) -> Vec<&'static Control> {
    STANDARD
        .iter()
        .chain(model.into_iter().flat_map(|m| m.controls.iter()))
        .collect()
}

/// Every control kiyoctl knows about, across every registered Model. For
/// `list-controls` and for telling "not on this camera" from "no such control".
pub fn every() -> Vec<&'static Control> {
    STANDARD
        .iter()
        .chain(MODELS.iter().flat_map(|m| m.controls.iter()))
        .collect()
}

/// Look a name up with no camera open. Use `Cam::find` when there is one.
pub fn find_any(name: &str) -> Option<&'static Control> {
    every().into_iter().find(|c| c.name == name)
}

/// True when this name belongs to a Model rather than the standard catalogue.
/// Used by profile migration to decide what moves into a per-camera section.
pub fn is_model_control(name: &str) -> bool {
    MODELS.iter().any(|m| m.controls.iter().any(|c| c.name == name))
}

impl Control {
    pub fn is_opaque(&self) -> bool {
        matches!(self.kind, Kind::Opaque(_))
    }

    /// Legal values, for help text and error messages.
    pub fn choices(&self) -> Option<Vec<&'static str>> {
        match &self.kind {
            Kind::Bool => Some(vec!["off", "on"]),
            Kind::Menu(m) => Some(m.iter().map(|(n, _)| *n).collect()),
            Kind::Opaque(o) => Some(o.iter().map(|o| o.name).collect()),
            Kind::Int { .. } => None,
        }
    }
}

/// Decode a little-endian control value.
fn decode(bytes: &[u8], signed: bool) -> i64 {
    let mut v: u64 = 0;
    for (i, b) in bytes.iter().enumerate() {
        v |= (*b as u64) << (8 * i);
    }
    if signed && !bytes.is_empty() {
        let bits = 8 * bytes.len() as u32;
        if bits < 64 && v & (1 << (bits - 1)) != 0 {
            return (v as i64) - (1i64 << bits);
        }
    }
    v as i64
}

/// Encode a value little-endian into `len` bytes.
fn encode(v: i64, len: usize) -> Vec<u8> {
    let u = v as u64;
    (0..len).map(|i| ((u >> (8 * i)) & 0xff) as u8).collect()
}

/// A control's current value plus, for numeric controls, its valid range.
pub struct Reading {
    pub value: String,
    pub range: Option<(i64, i64)>,
    pub step: Option<i64>,
    pub default: Option<String>,
    pub writable: bool,
}

/// Read a control from the camera. Returns Ok(None) when the camera does not
/// implement it, or when it is write-only (an extension unit).
pub fn read(cam: &Cam, ctrl: &Control) -> Result<Option<Reading>, String> {
    if ctrl.is_opaque() {
        return Ok(None);
    }
    let Some(info) = cam.info(ctrl.unit, ctrl.selector) else {
        return Ok(None);
    };
    if info & INFO_GET == 0 {
        return Ok(None);
    }
    let writable = info & INFO_SET != 0;
    let cur = cam.get(ctrl.unit, ctrl.selector, GET_CUR, ctrl.len)?;

    let render = |raw: i64| -> String {
        match &ctrl.kind {
            Kind::Bool => if raw != 0 { "on".into() } else { "off".into() },
            Kind::Menu(m) => m
                .iter()
                .find(|(_, v)| *v == raw)
                .map(|(n, _)| (*n).to_string())
                .unwrap_or_else(|| raw.to_string()),
            _ => raw.to_string(),
        }
    };

    let signed = matches!(ctrl.kind, Kind::Int { signed: true });
    let value = render(decode(&cur, signed));

    // Ranges are only meaningful for numeric controls, and cameras are free to
    // reject the query even for controls they do implement.
    let (range, step) = if matches!(ctrl.kind, Kind::Int { .. }) {
        let min = cam.get(ctrl.unit, ctrl.selector, GET_MIN, ctrl.len).ok().map(|b| decode(&b, signed));
        let max = cam.get(ctrl.unit, ctrl.selector, GET_MAX, ctrl.len).ok().map(|b| decode(&b, signed));
        let res = cam.get(ctrl.unit, ctrl.selector, GET_RES, ctrl.len).ok().map(|b| decode(&b, signed));
        (min.zip(max), res)
    } else {
        (None, None)
    };

    let default = cam
        .get(ctrl.unit, ctrl.selector, GET_DEF, ctrl.len)
        .ok()
        .map(|b| render(decode(&b, signed)));

    Ok(Some(Reading { value, range, step, default, writable }))
}

/// Write a control. `value` is the human-facing form: a number, `on`/`off`, or
/// a menu/option name.
pub fn write(cam: &Cam, ctrl: &Control, value: &str) -> Result<(), String> {
    let v = value.trim().to_lowercase();
    match &ctrl.kind {
        Kind::Opaque(opts) => {
            let opt = opts
                .iter()
                .find(|o| o.name == v)
                .ok_or_else(|| bad_value(ctrl, value))?;
            if let Some(pre) = opt.pre {
                cam.set(ctrl.unit, ctrl.selector, pre)?;
            }
            cam.set(ctrl.unit, ctrl.selector, opt.payload)
        }
        Kind::Bool => {
            let raw = match v.as_str() {
                "on" | "true" | "1" | "yes" => 1i64,
                "off" | "false" | "0" | "no" => 0i64,
                _ => return Err(bad_value(ctrl, value)),
            };
            cam.set(ctrl.unit, ctrl.selector, &encode(raw, ctrl.len))
        }
        Kind::Menu(m) => {
            let raw = m
                .iter()
                .find(|(n, _)| *n == v)
                .map(|(_, val)| *val)
                .or_else(|| v.parse::<i64>().ok())
                .ok_or_else(|| bad_value(ctrl, value))?;
            cam.set(ctrl.unit, ctrl.selector, &encode(raw, ctrl.len))
        }
        Kind::Int { .. } => {
            let raw: i64 = v.parse().map_err(|_| bad_value(ctrl, value))?;
            // Clamp rather than let the camera stall on an out-of-range write.
            let signed = matches!(ctrl.kind, Kind::Int { signed: true });
            let min = cam.get(ctrl.unit, ctrl.selector, GET_MIN, ctrl.len).ok().map(|b| decode(&b, signed));
            let max = cam.get(ctrl.unit, ctrl.selector, GET_MAX, ctrl.len).ok().map(|b| decode(&b, signed));
            if let (Some(lo), Some(hi)) = (min, max) {
                if raw < lo || raw > hi {
                    return Err(format!("{} must be between {lo} and {hi}", ctrl.name));
                }
            }
            cam.set(ctrl.unit, ctrl.selector, &encode(raw, ctrl.len))
        }
    }
}

fn bad_value(ctrl: &Control, value: &str) -> String {
    match ctrl.choices() {
        Some(c) => format!("invalid value '{value}' for {}; expected one of: {}", ctrl.name, c.join(", ")),
        None => format!("invalid value '{value}' for {}; expected a number", ctrl.name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_controls_report_their_option_names() {
        let hdr = find_any("hdr").expect("hdr must exist");
        assert!(hdr.is_opaque());
        assert_eq!(hdr.choices(), Some(vec!["off", "on"]));
    }

    #[test]
    fn standard_controls_are_not_opaque() {
        assert!(!find_any("brightness").unwrap().is_opaque());
        assert!(!find_any("auto_exposure").unwrap().is_opaque());
    }

    #[test]
    fn payloads_are_not_fixed_at_eight_bytes() {
        // The type must accept a payload of any length, so a vendor using
        // variable-length buffers can contribute without editing this file.
        const WIDE: &[OpaqueOpt] = &[OpaqueOpt {
            name: "wide",
            payload: &[0x01, 0x02, 0x03],
            pre: Some(&[0x00; 12]),
        }];
        assert_eq!(WIDE[0].payload.len(), 3);
        assert_eq!(WIDE[0].pre.unwrap().len(), 12);
    }

    /// The regression guard for the whole device-SPI refactor: the control
    /// list a Kiyo Pro sees must be byte-for-byte what 0.2.0 produced, in the
    /// same order. If this fails, the refactor changed user-visible behaviour.
    #[test]
    fn kiyo_pro_sees_exactly_what_it_saw_before() {
        const BEFORE: &[&str] = &[
            "brightness", "contrast", "saturation", "sharpness", "gamma", "gain",
            "hue", "backlight_compensation", "power_line_frequency",
            "white_balance_auto", "white_balance",
            "auto_exposure", "exposure_time", "focus_auto", "focus", "zoom",
            "hdr", "hdr_mode", "fov", "af_mode",
        ];
        let got: Vec<&str> = effective_controls(Some(&crate::models::razer_kiyo_pro::MODEL))
            .iter()
            .map(|c| c.name)
            .collect();
        assert_eq!(got, BEFORE);
    }

    #[test]
    fn a_camera_with_no_model_sees_only_standard_controls() {
        let got: Vec<&str> = effective_controls(None).iter().map(|c| c.name).collect();
        let want: Vec<&str> = STANDARD.iter().map(|c| c.name).collect();
        assert_eq!(got, want);
        assert!(!got.contains(&"hdr"));
    }

    #[test]
    fn every_includes_controls_from_models_that_are_not_attached() {
        let names: Vec<&str> = every().iter().map(|c| c.name).collect();
        assert!(names.contains(&"hdr"), "every() must list every registered Model's controls");
        assert!(names.contains(&"brightness"));
    }

    #[test]
    fn find_any_finds_model_controls_with_no_camera_open() {
        assert!(find_any("hdr").is_some());
        assert!(find_any("brightness").is_some());
        assert!(find_any("no_such_control").is_none());
    }

    #[test]
    fn is_model_control_distinguishes_the_two_catalogues() {
        assert!(is_model_control("hdr"));
        assert!(!is_model_control("brightness"));
        assert!(!is_model_control("no_such_control"));
    }
}
