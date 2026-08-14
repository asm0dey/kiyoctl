//! The catalogue of controls kiyoctl knows how to read and write.
//!
//! Standard UVC controls live on the camera terminal or processing unit and are
//! readable. Razer's extension-unit controls are write-only — the camera
//! accepts them but never reports them back — so their state is tracked in the
//! saved profile rather than queried from hardware.

use crate::device::{Cam, Unit, GET_CUR, GET_DEF, GET_MAX, GET_MIN, GET_RES, INFO_GET, INFO_SET};

/// A single option of a Razer extension-unit control.
pub struct RazerOpt {
    pub name: &'static str,
    /// Payload written to EU1_SET_ISP.
    pub payload: [u8; 8],
    /// Some options need a priming write first (the field-of-view ones do).
    pub pre: Option<[u8; 8]>,
}

pub enum Kind {
    Int { signed: bool },
    Bool,
    /// Named values over a small integer domain.
    Menu(&'static [(&'static str, i64)]),
    /// Razer extension unit: opaque payloads, no read-back.
    Razer(&'static [RazerOpt]),
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

/// Razer EU1 control selectors.
const EU1_SET_ISP: u8 = 0x01;

/// Persist the extension-unit state into the camera's own storage.
pub const RAZER_SAVE: [u8; 8] = [0xc0, 0x03, 0xa8, 0x00, 0x00, 0x00, 0x00, 0x00];

const HDR: &[RazerOpt] = &[
    RazerOpt { name: "off", payload: [0xff, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], pre: None },
    RazerOpt { name: "on",  payload: [0xff, 0x02, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00], pre: None },
];

const HDR_MODE: &[RazerOpt] = &[
    RazerOpt { name: "dark",   payload: [0xff, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], pre: None },
    RazerOpt { name: "bright", payload: [0xff, 0x07, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00], pre: None },
];

const FOV: &[RazerOpt] = &[
    RazerOpt { name: "wide", payload: [0xff, 0x01, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00], pre: None },
    RazerOpt {
        name: "medium",
        payload: [0xff, 0x01, 0x01, 0x03, 0x01, 0x00, 0x00, 0x00],
        pre: Some([0xff, 0x01, 0x00, 0x03, 0x01, 0x00, 0x00, 0x00]),
    },
    RazerOpt {
        name: "narrow",
        payload: [0xff, 0x01, 0x01, 0x03, 0x02, 0x00, 0x00, 0x00],
        pre: Some([0xff, 0x01, 0x00, 0x03, 0x02, 0x00, 0x00, 0x00]),
    },
];

const AF_MODE: &[RazerOpt] = &[
    RazerOpt { name: "responsive", payload: [0xff, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], pre: None },
    RazerOpt { name: "passive",    payload: [0xff, 0x06, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00], pre: None },
];

const POWER_LINE: &[(&str, i64)] =
    &[("disabled", 0), ("50hz", 1), ("60hz", 2), ("auto", 3)];

// UVC encodes the exposure mode as a bitmap, one bit per mode.
const AE_MODE: &[(&str, i64)] = &[
    ("manual", 1),
    ("auto", 2),
    ("shutter_priority", 4),
    ("aperture_priority", 8),
];

pub static CONTROLS: &[Control] = &[
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

    // --- Razer extension unit (write-only) -------------------------------
    Control { name: "hdr", unit: Unit::Razer, selector: EU1_SET_ISP, len: 8,
        kind: Kind::Razer(HDR), help: "high dynamic range", order: 2, requires: None },
    Control { name: "hdr_mode", unit: Unit::Razer, selector: EU1_SET_ISP, len: 8,
        kind: Kind::Razer(HDR_MODE), help: "HDR tone preference", order: 2, requires: None },
    Control { name: "fov", unit: Unit::Razer, selector: EU1_SET_ISP, len: 8,
        kind: Kind::Razer(FOV), help: "field of view", order: 2, requires: None },
    Control { name: "af_mode", unit: Unit::Razer, selector: EU1_SET_ISP, len: 8,
        kind: Kind::Razer(AF_MODE), help: "autofocus responsiveness", order: 2, requires: None },
];

pub fn find(name: &str) -> Option<&'static Control> {
    CONTROLS.iter().find(|c| c.name == name)
}

impl Control {
    pub fn is_razer(&self) -> bool {
        matches!(self.kind, Kind::Razer(_))
    }

    /// Legal values, for help text and error messages.
    pub fn choices(&self) -> Option<Vec<&'static str>> {
        match &self.kind {
            Kind::Bool => Some(vec!["off", "on"]),
            Kind::Menu(m) => Some(m.iter().map(|(n, _)| *n).collect()),
            Kind::Razer(o) => Some(o.iter().map(|o| o.name).collect()),
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
/// implement it, or when it is write-only (the Razer unit).
pub fn read(cam: &Cam, ctrl: &Control) -> Result<Option<Reading>, String> {
    if ctrl.is_razer() {
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
        Kind::Razer(opts) => {
            let opt = opts
                .iter()
                .find(|o| o.name == v)
                .ok_or_else(|| bad_value(ctrl, value))?;
            if let Some(pre) = &opt.pre {
                cam.set(ctrl.unit, ctrl.selector, pre)?;
            }
            cam.set(ctrl.unit, ctrl.selector, &opt.payload)
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
