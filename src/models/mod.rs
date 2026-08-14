//! The registry of cameras kiyoctl knows vendor-specific facts about.
//!
//! Adding a camera: copy `razer_kiyo_pro.rs`, change the ids, GUIDs and
//! payloads, and add one line to `MODELS`. See `docs/adding-a-camera.md` —
//! and do not guess payload bytes.

pub mod razer_kiyo_pro;

use crate::controls::Control;
use crate::usb::Unit;

pub struct Model {
    /// Shown in the TUI header, in `list`, and as a `list-controls` heading.
    pub name: &'static str,
    /// The vid:pid pairs this Model covers. Required, non-empty. This is the
    /// only thing matching looks at — see ADR 0002.
    pub usb_ids: &'static [(u16, u16)],
    /// Controls this camera adds on top of the standard UVC catalogue.
    pub controls: &'static [Control],
    /// Unit, selector and payload that tell the camera to keep its extension
    /// unit state across a power cycle. Issued once per operation that wrote
    /// at least one extension-unit control.
    pub persist: Option<(Unit, u8, &'static [u8])>,
}

pub static MODELS: &[Model] = &[razer_kiyo_pro::MODEL];

/// The Model covering this camera, if any. Exact vid:pid match; no ordering
/// significance, because no two Models may claim the same pair.
pub fn for_camera(vid: u16, pid: u16) -> Option<&'static Model> {
    MODELS.iter().find(|m| m.usb_ids.contains(&(vid, pid)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_model_declares_at_least_one_usb_id() {
        for m in MODELS {
            assert!(
                !m.usb_ids.is_empty(),
                "{} declares no usb_ids; matching is by vid:pid only (ADR 0002)",
                m.name
            );
        }
    }

    #[test]
    fn no_two_models_claim_the_same_camera() {
        let mut seen = HashSet::new();
        for m in MODELS {
            for id in m.usb_ids {
                assert!(
                    seen.insert(*id),
                    "{:04x}:{:04x} is claimed by more than one Model, including {}",
                    id.0, id.1, m.name
                );
            }
        }
    }

    #[test]
    fn no_model_repeats_a_control_name() {
        for m in MODELS {
            let mut seen = HashSet::new();
            for c in m.controls {
                assert!(seen.insert(c.name), "{} declares {} twice", m.name, c.name);
            }
        }
    }

    #[test]
    fn no_model_control_shadows_a_standard_one() {
        let standard: HashSet<&str> = crate::controls::STANDARD.iter().map(|c| c.name).collect();
        for m in MODELS {
            for c in m.controls {
                assert!(
                    !standard.contains(c.name),
                    "{} declares {}, which is already a standard UVC control",
                    m.name, c.name
                );
            }
        }
    }

    #[test]
    fn matching_is_by_usb_id() {
        let kiyo = for_camera(0x1532, 0x0e05).expect("the Kiyo Pro must match");
        assert_eq!(kiyo.name, "Razer Kiyo Pro");
        assert!(for_camera(0x046d, 0x085e).is_none(), "an unlisted camera must not match");
    }

    #[test]
    fn a_colliding_guid_does_not_select_a_model() {
        // The Dell UltraSharp WB7022 carries the byte-identical extension unit
        // GUID to the Kiyo Pro and takes entirely different payloads on it.
        // Matching must not select the Razer Model for it. See ADR 0002.
        assert!(
            for_camera(0x413c, 0xc015).is_none(),
            "Dell UltraSharp WB7022 must not match the Razer Model"
        );
    }

    #[test]
    fn every_model_control_lives_on_a_unit_the_model_can_address() {
        for m in MODELS {
            for c in m.controls {
                assert!(
                    matches!(c.unit, crate::usb::Unit::Extension(_)),
                    "{}'s {} is not on an extension unit; standard units belong in controls::STANDARD",
                    m.name, c.name
                );
            }
        }
    }
}
