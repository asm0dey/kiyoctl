//! TEMPLATE — copy this to src/models/<your_camera>.rs and fill it in.
//!
//! PROVENANCE (required): say where these bytes came from. One of:
//!   - "Dumped from my own hardware with a USB capture on <date>."
//!   - "From <project URL>, <licence>."
//! A Model with no provenance line will not be merged. This is also what keeps
//! kiyoctl's licence position auditable — see docs/adr/0001-relicense-to-gpl-3.md.
//!
//! DO NOT GUESS PAYLOAD BYTES. Every test in this repository passes on invented
//! payloads. If you do not have real bytes, open an issue with your
//! `kiyoctl probe` output instead of filling this in.

use crate::controls::{Control, Kind, OpaqueOpt};
use crate::models::Model;
use crate::usb::Unit;

/// The extension unit GUID, in *descriptor byte order*.
///
/// This is not the order vendors print it in. A GUID published as
/// `23e49ed0-1178-4f31-ae52-d2fb8a8d3b48` is stored with its first three
/// fields byte-reversed: d0 9e e4 23, 78 11, 31 4f, then ae 52 and the rest
/// as-is. `kiyoctl probe` prints the published form; reverse it as shown.
pub const MY_UNIT_GUID: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

const UNIT: Unit = Unit::Extension(&MY_UNIT_GUID);

/// Some cameras put every control behind one selector, as the Kiyo Pro does.
/// Others give each control its own. Both work: `selector` is per-Control.
const SET_SELECTOR: u8 = 0x01;

/// Named payloads for a control the camera will not read back.
///
/// If your camera *does* answer GET_CUR on this unit — `kiyoctl probe` will
/// tell you — do not use Kind::Opaque. Declare Kind::Int, Bool or Menu on
/// Unit::Extension instead and the ordinary read path just works.
const MY_CONTROL: &[OpaqueOpt] = &[
    OpaqueOpt { name: "off", payload: &[0x00], pre: None },
    OpaqueOpt { name: "on",  payload: &[0x01], pre: None },
];

static CONTROLS: &[Control] = &[
    Control {
        // Must not collide with a standard UVC control name — the registry
        // self-check test enforces this.
        name: "my_control",
        unit: UNIT,
        selector: SET_SELECTOR,
        len: 1,
        kind: Kind::Opaque(MY_CONTROL),
        help: "what a user should understand this to do",
        // 0 = gates other controls, 1 = ordinary, 2 = apply last.
        order: 2,
        // Some(("other_control", &["off"])) when this is only writable while
        // another control holds one of those values.
        requires: None,
    },
];

pub static MODEL: Model = Model {
    name: "Vendor Model Name",
    // Required, non-empty. Matching is by vid:pid only — GUIDs collide
    // across vendors. See docs/adr/0002-match-models-by-usb-id.md.
    usb_ids: &[(0x0000, 0x0000)],
    controls: CONTROLS,
    // The write that makes extension-unit settings survive a power cycle,
    // if your camera has one. None is fine.
    persist: None,
};
