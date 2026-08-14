//! Razer Kiyo Pro — 1532:0e05.
//!
//! Provenance: the extension unit GUID, selector and payloads were worked out
//! by kiyoproctrls (https://github.com/soyersoyer/kiyoproctrls), MIT licensed,
//! and verified against real hardware by this repository's author.

use crate::controls::{Control, Kind, OpaqueOpt};
use crate::models::Model;
use crate::usb::Unit;

/// Razer's ISP extension unit, GUID 23e49ed0-1178-4f31-ae52-d2fb8a8d3b48.
///
/// Note that the Dell UltraSharp WB7022 carries this same GUID and takes
/// entirely different payloads on it, which is why Models match on vid:pid.
pub const EU1_GUID: [u8; 16] = [
    0xd0, 0x9e, 0xe4, 0x23, 0x78, 0x11, 0x31, 0x4f,
    0xae, 0x52, 0xd2, 0xfb, 0x8a, 0x8d, 0x3b, 0x48,
];

const UNIT: Unit = Unit::Extension(&EU1_GUID);

/// Every control on this unit uses the one "set ISP" selector.
const EU1_SET_ISP: u8 = 0x01;

/// Persist the extension-unit state into the camera's own storage.
const SAVE: &[u8] = &[0xc0, 0x03, 0xa8, 0x00, 0x00, 0x00, 0x00, 0x00];

const HDR: &[OpaqueOpt] = &[
    OpaqueOpt { name: "off", payload: &[0xff, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], pre: None },
    OpaqueOpt { name: "on",  payload: &[0xff, 0x02, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00], pre: None },
];

const HDR_MODE: &[OpaqueOpt] = &[
    OpaqueOpt { name: "dark",   payload: &[0xff, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], pre: None },
    OpaqueOpt { name: "bright", payload: &[0xff, 0x07, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00], pre: None },
];

const FOV: &[OpaqueOpt] = &[
    OpaqueOpt { name: "wide", payload: &[0xff, 0x01, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00], pre: None },
    OpaqueOpt {
        name: "medium",
        payload: &[0xff, 0x01, 0x01, 0x03, 0x01, 0x00, 0x00, 0x00],
        pre: Some(&[0xff, 0x01, 0x00, 0x03, 0x01, 0x00, 0x00, 0x00]),
    },
    OpaqueOpt {
        name: "narrow",
        payload: &[0xff, 0x01, 0x01, 0x03, 0x02, 0x00, 0x00, 0x00],
        pre: Some(&[0xff, 0x01, 0x00, 0x03, 0x02, 0x00, 0x00, 0x00]),
    },
];

const AF_MODE: &[OpaqueOpt] = &[
    OpaqueOpt { name: "responsive", payload: &[0xff, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], pre: None },
    OpaqueOpt { name: "passive",    payload: &[0xff, 0x06, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00], pre: None },
];

static CONTROLS: &[Control] = &[
    Control { name: "hdr", unit: UNIT, selector: EU1_SET_ISP, len: 8,
        kind: Kind::Opaque(HDR), help: "high dynamic range", order: 2, requires: None },
    Control { name: "hdr_mode", unit: UNIT, selector: EU1_SET_ISP, len: 8,
        kind: Kind::Opaque(HDR_MODE), help: "HDR tone preference", order: 2, requires: None },
    Control { name: "fov", unit: UNIT, selector: EU1_SET_ISP, len: 8,
        kind: Kind::Opaque(FOV), help: "field of view", order: 2, requires: None },
    Control { name: "af_mode", unit: UNIT, selector: EU1_SET_ISP, len: 8,
        kind: Kind::Opaque(AF_MODE), help: "autofocus responsiveness", order: 2, requires: None },
];

/// A `const` rather than a `static` so `MODELS` can embed it by value.
pub const MODEL: Model = Model {
    name: "Razer Kiyo Pro",
    usb_ids: &[(0x1532, 0x0e05)],
    controls: CONTROLS,
    persist: Some((UNIT, EU1_SET_ISP, SAVE)),
};
