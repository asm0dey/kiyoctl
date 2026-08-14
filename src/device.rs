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

/// Razer's ISP extension unit, GUID 23e49ed0-1178-4f31-ae52-d2fb8a8d3b48.
pub const RAZER_EU1_GUID: [u8; 16] = [
    0xd0, 0x9e, 0xe4, 0x23, 0x78, 0x11, 0x31, 0x4f, 0xae, 0x52, 0xd2, 0xfb, 0x8a, 0x8d, 0x3b, 0x48,
];

const TIMEOUT: Duration = Duration::from_millis(1000);

/// Which logical unit inside the camera a control lives on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unit {
    /// Camera terminal: optics — exposure, focus, zoom.
    Camera,
    /// Processing unit: image pipeline — brightness, white balance, gain.
    Processing,
    /// Razer ISP extension unit: HDR, field of view, AF tuning.
    Razer,
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
    razer_id: Option<u8>,
}

/// What a scan found: enough to identify a camera without opening it.
pub struct Found {
    pub name: String,
    pub vid: u16,
    pub pid: u16,
    pub bus: u8,
    pub address: u8,
    pub has_razer: bool,
}

/// Units parsed out of a VideoControl interface descriptor.
struct Units {
    vc_iface: u8,
    camera_id: Option<u8>,
    processing_id: Option<u8>,
    razer_id: Option<u8>,
}

/// Walk the class-specific descriptors of a VideoControl interface, collecting
/// the unit IDs we know how to talk to.
fn parse_units(dev: &Device<GlobalContext>) -> Option<Units> {
    let config = dev.active_config_descriptor().ok()?;
    for iface in config.interfaces() {
        for desc in iface.descriptors() {
            if desc.class_code() != CC_VIDEO || desc.sub_class_code() != SC_VIDEOCONTROL {
                continue;
            }
            let mut units = Units {
                vc_iface: desc.interface_number(),
                camera_id: None,
                processing_id: None,
                razer_id: None,
            };
            let extra = desc.extra();
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
                            if block[4..20] == RAZER_EU1_GUID {
                                units.razer_id = Some(block[3]);
                            }
                        }
                        _ => {}
                    }
                }
                i += len;
            }
            return Some(units);
        }
    }
    None
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
        let Some(units) = parse_units(&dev) else {
            continue;
        };
        let desc = dev.device_descriptor()?;
        // Opening is what yields a product string; a camera we cannot open is
        // still worth listing, just with a fallback name.
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
            has_razer: units.razer_id.is_some(),
        });
        let _ = units.vc_iface;
    }
    Ok(out)
}

impl Cam {
    /// Open a camera. With no selector, opens the only camera present; if
    /// several are attached the caller must disambiguate.
    pub fn open(selector: Option<&str>) -> Result<Cam, String> {
        let mut candidates = Vec::new();
        for dev in rusb::devices().map_err(|e| e.to_string())?.iter() {
            let Some(units) = parse_units(&dev) else {
                continue;
            };
            let desc = dev.device_descriptor().map_err(|e| e.to_string())?;
            candidates.push((dev, desc, units));
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

        let (dev, desc, units) = chosen;
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
            vc_iface: units.vc_iface,
            camera_id: units.camera_id,
            processing_id: units.processing_id,
            razer_id: units.razer_id,
        })
    }

    pub fn has_razer_unit(&self) -> bool {
        self.razer_id.is_some()
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
            Unit::Razer => self.razer_id,
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
            .ok_or_else(|| format!("{unit:?} unit not present on this camera"))?;
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
            .ok_or_else(|| format!("{unit:?} unit not present on this camera"))?;
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
