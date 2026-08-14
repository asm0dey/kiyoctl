//! USB Video Class device discovery and control-endpoint access.
//!
//! UVC controls are class-specific requests on the default control pipe, so we
//! never claim an interface and never fight the system's camera driver.

use rusb::{Device, DeviceHandle, GlobalContext};
use std::time::Duration;

/// UVC request codes (UVC 1.5 §4.2).
pub const SET_CUR: u8 = 0x01;
pub const GET_CUR: u8 = 0x81;
pub const GET_MIN: u8 = 0x82;
pub const GET_MAX: u8 = 0x83;
pub const GET_RES: u8 = 0x84;
pub const GET_DEF: u8 = 0x87;
pub const GET_INFO: u8 = 0x86;

/// GET_INFO capability bits.
pub const INFO_GET: u8 = 0x01;
pub const INFO_SET: u8 = 0x02;

const CC_VIDEO: u8 = 0x0e;
const SC_VIDEOCONTROL: u8 = 0x01;
const CS_INTERFACE: u8 = 0x24;
const VC_INPUT_TERMINAL: u8 = 0x02;
const VC_PROCESSING_UNIT: u8 = 0x05;
const VC_EXTENSION_UNIT: u8 = 0x06;
const ITT_CAMERA: u16 = 0x0201;

const TIMEOUT: Duration = Duration::from_millis(1000);

/// Which logical unit inside the camera a control lives on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unit {
    /// Camera terminal: optics — exposure, focus, zoom.
    Camera,
    /// Processing unit: image pipeline — brightness, white balance, gain.
    Processing,
    /// A vendor extension unit, addressed by its GUID. A camera may carry
    /// several; a Model's controls may name more than one.
    Extension(&'static [u8; 16]),
}

impl Unit {
    /// For error messages. `Debug` on an Extension would print sixteen bytes.
    pub fn label(&self) -> String {
        match self {
            Unit::Camera => "camera terminal".to_string(),
            Unit::Processing => "processing unit".to_string(),
            Unit::Extension(g) => format!("extension unit {}", format_guid(g)),
        }
    }
}

/// Shown when the camera is enumerated but will not answer control requests.
/// The Kiyo Pro is known to wedge like this; only a replug clears it.
pub const NOT_RESPONDING: &str =
    "the camera is attached but is not answering USB control requests — \
     unplug it and plug it back in, then try again";

pub struct Cam {
    handle: DeviceHandle<GlobalContext>,
    pub name: String,
    pub vid: u16,
    pub pid: u16,
    /// False when even a standard descriptor request fails.
    pub responding: bool,
    vc_iface: u8,
    camera_id: Option<u8>,
    processing_id: Option<u8>,
    extension_units: Vec<([u8; 16], u8)>,
    /// The Model covering this camera, if any. Selected by vid:pid.
    pub model: Option<&'static crate::models::Model>,
}

/// What a scan found: enough to identify a camera without opening it.
pub struct Found {
    pub name: String,
    pub vid: u16,
    pub pid: u16,
    pub bus: u8,
    pub address: u8,
    /// Every extension unit GUID this camera carries, in descriptor order.
    pub extension_guids: Vec<[u8; 16]>,
}

/// Units parsed out of a VideoControl interface descriptor.
pub struct ParsedUnits {
    pub camera_id: Option<u8>,
    pub processing_id: Option<u8>,
    /// Every extension unit present, as (GUID, unit id), in descriptor order.
    pub extensions: Vec<([u8; 16], u8)>,
}

/// Walk the class-specific descriptor bytes of a VideoControl interface.
///
/// Pure: takes the raw `extra` block so it can be tested without hardware.
fn parse_extra(extra: &[u8]) -> ParsedUnits {
    let mut units = ParsedUnits {
        camera_id: None,
        processing_id: None,
        extensions: Vec::new(),
    };
    let mut i = 0usize;
    while i + 3 <= extra.len() {
        let len = extra[i] as usize;
        if len < 3 || i + len > extra.len() {
            break;
        }
        let block = &extra[i..i + len];
        if block[1] == CS_INTERFACE {
            match block[2] {
                // Input terminal is only a camera if its type says so.
                VC_INPUT_TERMINAL if len >= 6 => {
                    let ttype = u16::from_le_bytes([block[4], block[5]]);
                    if ttype == ITT_CAMERA {
                        units.camera_id = Some(block[3]);
                    }
                }
                VC_PROCESSING_UNIT if len >= 4 => units.processing_id = Some(block[3]),
                VC_EXTENSION_UNIT if len >= 20 => {
                    let mut guid = [0u8; 16];
                    guid.copy_from_slice(&block[4..20]);
                    units.extensions.push((guid, block[3]));
                }
                _ => {}
            }
        }
        i += len;
    }
    units
}

/// The VideoControl interface number plus the units behind it.
struct Interface {
    vc_iface: u8,
    units: ParsedUnits,
}

fn parse_units(dev: &Device<GlobalContext>) -> Option<Interface> {
    let config = dev.active_config_descriptor().ok()?;
    for iface in config.interfaces() {
        for desc in iface.descriptors() {
            if desc.class_code() != CC_VIDEO || desc.sub_class_code() != SC_VIDEOCONTROL {
                continue;
            }
            return Some(Interface {
                vc_iface: desc.interface_number(),
                units: parse_extra(desc.extra()),
            });
        }
    }
    None
}

/// Render a descriptor GUID the way vendors publish it. The first three fields
/// are stored little-endian in the descriptor; the last two are not.
pub fn format_guid(b: &[u8; 16]) -> String {
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{}",
        b[3], b[2], b[1], b[0],
        b[5], b[4],
        b[7], b[6],
        b[8], b[9],
        b[10..16].iter().map(|x| format!("{x:02x}")).collect::<String>()
    )
}

/// A cheap signature of the UVC cameras currently attached.
///
/// Built from descriptors alone — no device is opened — so the daemon can poll
/// this continuously and only reach for the camera when something changes.
pub fn fingerprint() -> String {
    let Ok(devices) = rusb::devices() else {
        return String::new();
    };
    let mut ids: Vec<String> = devices
        .iter()
        .filter(|d| parse_units(d).is_some())
        .filter_map(|d| {
            let desc = d.device_descriptor().ok()?;
            Some(format!(
                "{:04x}:{:04x}@{}.{}",
                desc.vendor_id(),
                desc.product_id(),
                d.bus_number(),
                d.address()
            ))
        })
        .collect();
    ids.sort();
    ids.join(",")
}

/// List every UVC camera attached to the system.
pub fn scan() -> rusb::Result<Vec<Found>> {
    let mut out = Vec::new();
    for dev in rusb::devices()?.iter() {
        let Some(iface) = parse_units(&dev) else {
            continue;
        };
        let desc = dev.device_descriptor()?;
        let name = dev
            .open()
            .ok()
            .and_then(|h| h.read_product_string_ascii(&desc).ok())
            .unwrap_or_else(|| format!("UVC camera {:04x}:{:04x}", desc.vendor_id(), desc.product_id()));
        out.push(Found {
            name,
            vid: desc.vendor_id(),
            pid: desc.product_id(),
            bus: dev.bus_number(),
            address: dev.address(),
            extension_guids: iface.units.extensions.iter().map(|(g, _)| *g).collect(),
        });
    }
    Ok(out)
}

/// Resolve a Unit to the unit id to put in wIndex. Pure, so the "absent GUID
/// must not fall back to another unit" case is testable without hardware.
fn find_unit(present: &[([u8; 16], u8)], unit: Unit) -> Option<u8> {
    match unit {
        Unit::Extension(guid) => present
            .iter()
            .find(|(g, _)| g == guid)
            .map(|(_, id)| *id),
        // The fixed units are fields on Cam, not entries in this list.
        Unit::Camera | Unit::Processing => None,
    }
}

impl Cam {
    /// Open a camera. With no selector, opens the only camera present; if
    /// several are attached the caller must disambiguate.
    pub fn open(selector: Option<&str>) -> Result<Cam, String> {
        let mut candidates = Vec::new();
        for dev in rusb::devices().map_err(|e| e.to_string())?.iter() {
            let Some(iface) = parse_units(&dev) else {
                continue;
            };
            let desc = dev.device_descriptor().map_err(|e| e.to_string())?;
            candidates.push((dev, desc, iface));
        }
        if candidates.is_empty() {
            return Err("no UVC camera found".into());
        }

        // A selector matches "vvvv:pppp" exactly, or any case-insensitive
        // substring of the product name.
        let chosen = match selector {
            None => {
                if candidates.len() > 1 {
                    let names: Vec<String> = candidates
                        .iter()
                        .map(|(_, d, _)| format!("{:04x}:{:04x}", d.vendor_id(), d.product_id()))
                        .collect();
                    return Err(format!(
                        "several cameras attached ({}); pass --device",
                        names.join(", ")
                    ));
                }
                candidates.remove(0)
            }
            Some(sel) => {
                let sel_lower = sel.to_lowercase();
                let mut pick = None;
                for (idx, (dev, desc, _)) in candidates.iter().enumerate() {
                    let id = format!("{:04x}:{:04x}", desc.vendor_id(), desc.product_id());
                    let name = dev
                        .open()
                        .ok()
                        .and_then(|h| h.read_product_string_ascii(desc).ok())
                        .unwrap_or_default()
                        .to_lowercase();
                    if id == sel_lower || (!name.is_empty() && name.contains(&sel_lower)) {
                        pick = Some(idx);
                        break;
                    }
                }
                match pick {
                    Some(i) => candidates.remove(i),
                    None => return Err(format!("no camera matching '{sel}'")),
                }
            }
        };

        let (dev, desc, iface) = chosen;
        let handle = dev.open().map_err(|e| match e {
            rusb::Error::Access => "permission denied opening the camera".to_string(),
            other => format!("cannot open camera: {other}"),
        })?;
        let name = handle
            .read_product_string_ascii(&desc)
            .unwrap_or_else(|_| format!("UVC camera {:04x}:{:04x}", desc.vendor_id(), desc.product_id()));

        // A string-descriptor read is the cheapest request every working USB
        // device answers; if even that fails the device has stopped talking.
        let responding = handle.read_languages(TIMEOUT).is_ok();

        Ok(Cam {
            handle,
            name,
            vid: desc.vendor_id(),
            pid: desc.product_id(),
            responding,
            vc_iface: iface.vc_iface,
            camera_id: iface.units.camera_id,
            processing_id: iface.units.processing_id,
            extension_units: iface.units.extensions,
            model: crate::models::for_camera(desc.vendor_id(), desc.product_id()),
        })
    }

    // ponytail: transitional. "Does this camera have vendor controls" is now
    // "does it have a Model"; the last callers go in Tasks 8-9, which delete
    // this method. Keeping it here is a smaller diff than rewriting six call
    // sites those tasks are about to rewrite anyway.
    pub fn has_razer_unit(&self) -> bool {
        self.model.is_some()
    }

    /// Replace a low-level transfer error with something actionable when the
    /// device has stopped answering altogether.
    pub fn explain(&self, e: String) -> String {
        if self.responding {
            e
        } else {
            NOT_RESPONDING.to_string()
        }
    }

    fn unit_id(&self, unit: Unit) -> Option<u8> {
        match unit {
            Unit::Camera => self.camera_id,
            Unit::Processing => self.processing_id,
            Unit::Extension(_) => find_unit(&self.extension_units, unit),
        }
    }

    /// wIndex packs the unit and the interface it lives behind.
    fn windex(&self, unit_id: u8) -> u16 {
        ((unit_id as u16) << 8) | self.vc_iface as u16
    }

    /// Issue a UVC GET request. `len` must match the control's data length.
    pub fn get(&self, unit: Unit, selector: u8, request: u8, len: usize) -> Result<Vec<u8>, String> {
        let unit_id = self
            .unit_id(unit)
            .ok_or_else(|| format!("{} not present on this camera", unit.label()))?;
        let mut buf = vec![0u8; len];
        let n = self
            .handle
            .read_control(0xA1, request, (selector as u16) << 8, self.windex(unit_id), &mut buf, TIMEOUT)
            .map_err(|e| e.to_string())?;
        buf.truncate(n);
        Ok(buf)
    }

    /// Issue a UVC SET_CUR request.
    pub fn set(&self, unit: Unit, selector: u8, data: &[u8]) -> Result<(), String> {
        let unit_id = self
            .unit_id(unit)
            .ok_or_else(|| format!("{} not present on this camera", unit.label()))?;
        if std::env::var_os("CAMCTL_DEBUG").is_some() {
            eprintln!("SET_CUR {unit:?} sel={selector:#04x} data={data:02x?}");
        }
        self.handle
            .write_control(0x21, SET_CUR, (selector as u16) << 8, self.windex(unit_id), data, TIMEOUT)
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Capability bits for a control, or None if the camera rejects the query
    /// (which is how unsupported controls present themselves).
    pub fn info(&self, unit: Unit, selector: u8) -> Option<u8> {
        if self.unit_id(unit).is_none() {
            return None;
        }
        self.get(unit, selector, GET_INFO, 1).ok().map(|b| b[0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal VideoControl class-specific descriptor block: one camera
    /// input terminal, one processing unit, two extension units.
    fn sample_extra() -> Vec<u8> {
        let mut v = Vec::new();
        // Input terminal, id 1, type ITT_CAMERA (0x0201)
        v.extend_from_slice(&[6, CS_INTERFACE, VC_INPUT_TERMINAL, 1, 0x01, 0x02]);
        // Input terminal, id 9, type 0x0101 (streaming) — must be ignored
        v.extend_from_slice(&[6, CS_INTERFACE, VC_INPUT_TERMINAL, 9, 0x01, 0x01]);
        // Processing unit, id 2
        v.extend_from_slice(&[4, CS_INTERFACE, VC_PROCESSING_UNIT, 2]);
        // Extension unit, id 3, Razer's GUID
        let mut eu = vec![20, CS_INTERFACE, VC_EXTENSION_UNIT, 3];
        eu.extend_from_slice(&crate::models::razer_kiyo_pro::EU1_GUID);
        v.extend_from_slice(&eu);
        // Extension unit, id 4, some other GUID
        let mut eu2 = vec![20, CS_INTERFACE, VC_EXTENSION_UNIT, 4];
        eu2.extend_from_slice(&[0xaa; 16]);
        v.extend_from_slice(&eu2);
        v
    }

    #[test]
    fn parses_every_unit_kind() {
        let p = parse_extra(&sample_extra());
        assert_eq!(p.camera_id, Some(1), "streaming terminal must not win");
        assert_eq!(p.processing_id, Some(2));
        assert_eq!(p.extensions.len(), 2, "both extension units must be returned");
        assert_eq!(p.extensions[0], (crate::models::razer_kiyo_pro::EU1_GUID, 3));
        assert_eq!(p.extensions[1], ([0xaa; 16], 4));
    }

    #[test]
    fn truncated_descriptor_does_not_panic() {
        let full = sample_extra();
        for cut in 0..full.len() {
            let _ = parse_extra(&full[..cut]);
        }
    }

    #[test]
    fn zero_length_block_does_not_loop_forever() {
        // A malformed block claiming length 0 must terminate the walk.
        let p = parse_extra(&[0, CS_INTERFACE, VC_PROCESSING_UNIT, 7]);
        assert_eq!(p.processing_id, None);
    }

    #[test]
    fn a_control_on_an_absent_unit_resolves_to_no_unit_id() {
        let present = [(crate::models::razer_kiyo_pro::EU1_GUID, 3u8)];
        assert_eq!(
            find_unit(&present, Unit::Extension(&crate::models::razer_kiyo_pro::EU1_GUID)),
            Some(3)
        );
        assert_eq!(
            find_unit(&present, Unit::Extension(&[0xaa; 16])),
            None,
            "an absent GUID must not fall back to unit zero or to another unit"
        );
    }

    #[test]
    fn guid_renders_in_canonical_form() {
        assert_eq!(
            format_guid(&crate::models::razer_kiyo_pro::EU1_GUID),
            "23e49ed0-1178-4f31-ae52-d2fb8a8d3b48",
            "first three fields are little-endian in the descriptor"
        );
    }
}
