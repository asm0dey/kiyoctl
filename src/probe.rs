//! `kiyoctl probe` — read-only reconnaissance of a camera's extension units.
//!
//! Reports which selectors exist, what they will admit to, and what they
//! currently hold. It cannot tell you what any of it *means*: that takes a
//! USB capture of the vendor's own application. See docs/adding-a-camera.md.

use crate::usb::{format_guid, Cam, GET_CUR, GET_LEN, INFO_GET, INFO_SET};

pub struct Selector {
    pub selector: u8,
    /// GET_INFO capability bits.
    pub info: u8,
    /// GET_LEN, when the camera answers it.
    pub len: Option<u16>,
    /// GET_CUR bytes, when the selector is readable.
    pub current: Option<Vec<u8>>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

pub fn report(guid: &[u8; 16], vid: u16, pid: u16, found: &[Selector]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "extension unit {}  on {vid:04x}:{pid:04x}\n",
        format_guid(guid)
    ));
    if found.is_empty() {
        out.push_str("  no selector answered GET_INFO\n");
        return out;
    }
    for s in found {
        let caps = match (s.info & INFO_GET != 0, s.info & INFO_SET != 0) {
            (true, true) => "get set",
            (true, false) => "get",
            (false, true) => "write-only",
            (false, false) => "neither",
        };
        let len = s.len.map(|l| l.to_string()).unwrap_or_else(|| "?".into());
        let value = match &s.current {
            Some(b) => hex(b),
            None => "-".to_string(),
        };
        out.push_str(&format!(
            "  selector 0x{:02x}  info 0x{:02x} ({caps})  len {len}  {value}\n",
            s.selector, s.info
        ));
    }
    out
}

/// Walk every selector on every extension unit. Read-only: GET_INFO, GET_LEN
/// and GET_CUR only, never SET_CUR.
pub fn run(cam: &Cam) -> String {
    let mut out = String::new();
    let guids = cam.extension_unit_guids();
    if guids.is_empty() {
        return format!("{} has no extension units.\n", cam.name);
    }
    for guid in guids {
        // `Box::leak` is how a runtime-discovered GUID reaches a
        // `Unit::Extension(&'static ...)`. It leaks 16 bytes per unit, once,
        // in a command that exits immediately — acceptable here and nowhere
        // else.
        let unit = crate::usb::Unit::Extension(Box::leak(Box::new(guid)));
        let mut found = Vec::new();
        for selector in 1u8..=0xff {
            let Some(info) = cam.info(unit, selector) else { continue };
            if info == 0 {
                continue;
            }
            let len = cam
                .get(unit, selector, GET_LEN, 2)
                .ok()
                .filter(|b| b.len() == 2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]));
            let current = if info & INFO_GET != 0 {
                len.and_then(|l| cam.get(unit, selector, GET_CUR, l as usize).ok())
            } else {
                None
            };
            found.push(Selector { selector, info, len, current });
        }
        out.push_str(&report(&guid, cam.vid, cam.pid, &found));
        out.push('\n');
    }
    out.push_str(
        "Paste this into an issue to have your camera's support added.\n\
         Selector numbers and current values are facts; what they mean is not\n\
         derivable from this output. See docs/adding-a-camera.md.\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_readable_selector_shows_its_bytes() {
        let found = vec![Selector {
            selector: 0x01,
            info: 0x03,
            len: Some(8),
            current: Some(vec![0xff, 0x02, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00]),
        }];
        let out = report(&[0xd0, 0x9e, 0xe4, 0x23, 0x78, 0x11, 0x31, 0x4f,
                           0xae, 0x52, 0xd2, 0xfb, 0x8a, 0x8d, 0x3b, 0x48],
                         0x1532, 0x0e05, &found);
        assert!(out.contains("23e49ed0-1178-4f31-ae52-d2fb8a8d3b48"), "canonical GUID");
        assert!(out.contains("1532:0e05"));
        assert!(out.contains("0x01"));
        assert!(out.contains("ff 02 01 00 00 00 00 00"));
        assert!(out.contains("get set"), "capability bits must be spelled out");
    }

    #[test]
    fn a_write_only_selector_says_so_instead_of_showing_nothing() {
        let found = vec![Selector { selector: 0x02, info: 0x02, len: Some(8), current: None }];
        let out = report(&[0u8; 16], 0x1532, 0x0e05, &found);
        assert!(out.contains("write-only"), "a selector that refuses GET_CUR must be labelled");
        assert!(!out.contains("get set"));
    }

    #[test]
    fn a_unit_that_answers_nothing_says_so() {
        let out = report(&[0u8; 16], 0x1532, 0x0e05, &[]);
        assert!(out.contains("no selector answered"));
    }
}
