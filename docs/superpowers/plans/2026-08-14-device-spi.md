# Device SPI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every vendor-specific fact about a camera a data structure in one file, so a camera kiyoctl does not support can be added by writing that file and one registry line.

**Architecture:** A `Model` const per camera, selected by USB vid:pid, holding controls whose `Unit::Extension(guid)` addresses one of the camera's extension units by GUID. The transport layer becomes vendor-neutral; the thirty-nine `is_razer()` call sites split into "cannot be read back" (`is_opaque()`) and "this Model declares it". Profiles grow a per-camera section so vendor values are never lost, with a load-time migration so existing profiles keep working.

**Tech Stack:** Rust 2021, `rusb` 0.9 (libusb), `clap` 4.6 (derive), `ratatui` 0.30, `serde`/`serde_json`, `insta` 1.48 for snapshot tests. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-08-14-device-spi-design.md`

## Global Constraints

- **No new dependencies.** Nothing may be added to `Cargo.toml` `[dependencies]` or `[dev-dependencies]`.
- **`cargo build` and `cargo test` must pass with no new warnings** at the end of every task.
- **The eight `insta` snapshots in `src/snapshots/` must not move.** A moved snapshot means a real behaviour change: stop and report it rather than running `cargo insta accept`.
- **No downgrades for supported behaviour** (ADR 0004). A Kiyo Pro owner upgrading must lose nothing. The Dell UltraSharp WB7022 is the one deliberate exception.
- **Licence is GPL-3.0-or-later** from Task 1 onward (ADR 0001).
- **Matching is by vid:pid only** (ADR 0002). No code may select a Model by GUID.
- **Every file under `src/models/` carries a provenance comment** naming where its bytes came from.
- Platform is macOS. `cargo test` runs without a camera attached; no test may open USB.
- Commit after every task. Commit messages use the `feat:` / `fix:` / `refactor:` / `docs:` / `chore:` prefixes already in this repo's history.

---

## File Structure

| File | Responsibility |
| --- | --- |
| `src/usb.rs` | *(renamed from `device.rs`)* USB/UVC transport: `Cam`, `Unit`, `scan`, `fingerprint`, control requests, descriptor parsing |
| `src/models/mod.rs` | `Model` struct, the `MODELS` registry, vid:pid matching, registry self-check tests |
| `src/models/razer_kiyo_pro.rs` | The Kiyo Pro's GUID, payloads, controls, persist triple |
| `src/models/_template.rs` | Commented skeleton for a contributor. Not compiled. |
| `src/controls.rs` | `Control`, `Kind`, `STANDARD`, `effective_controls`, `every`, `find_any`, read/write |
| `src/profile.rs` | `Profile` with per-camera sections, migration, capture, apply |
| `src/probe.rs` | The `probe` command's selector walk and its report formatting |
| `src/main.rs` | CLI wiring and command bodies |
| `src/tui.rs` | Terminal UI |
| `docs/adding-a-camera.md` | Contributor instructions |

---

## Task 1: Relicense to GPL-3.0-or-later

Isolated from all code changes so a licence question never blocks a refactor review.

**Files:**
- Modify: `LICENSE` (replace entirely)
- Modify: `Cargo.toml:5`
- Modify: `README.md:284-286`

**Interfaces:**
- Consumes: nothing
- Produces: nothing consumed by later tasks

- [ ] **Step 1: Replace the licence text**

Fetch the canonical GPL-3.0 text and write it to `LICENSE`, replacing the MIT text:

```bash
curl -fsSL https://www.gnu.org/licenses/gpl-3.0.txt -o LICENSE
```

Verify it starts with `GNU GENERAL PUBLIC LICENSE` and `Version 3, 29 June 2007`, and that it is roughly 35 KB. If the fetch fails, stop — do not hand-write licence text.

- [ ] **Step 2: Update the crate metadata**

In `Cargo.toml`, change line 5 from `license = "MIT"` to:

```toml
license = "GPL-3.0-or-later"
```

- [ ] **Step 3: Update the README**

Replace the `## License` section at `README.md:284-286` with:

```markdown
## License

GPL-3.0-or-later — see [LICENSE](LICENSE).

Released 0.1.1 and 0.2.0 were MIT and remain so. The change to GPL-3.0-or-later
is what lets kiyoctl accept camera support contributed from LGPL-3.0 sources —
see [docs/adr/0001-relicense-to-gpl-3.md](docs/adr/0001-relicense-to-gpl-3.md).
```

- [ ] **Step 4: Verify the build still passes**

Run: `cargo build 2>&1 | tail -20`
Expected: `Finished` with no warnings. Cargo validates the SPDX identifier, so a typo in the licence string fails here.

- [ ] **Step 5: Commit**

```bash
git add LICENSE Cargo.toml README.md
git commit -m "chore: relicense to GPL-3.0-or-later

Lets kiyoctl accept camera Models contributed from LGPL-3.0 sources.
See docs/adr/0001-relicense-to-gpl-3.md."
```

---

## Task 2: Rename `src/device.rs` to `src/usb.rs`

Purely mechanical and fully compiler-verified. Doing it alone means the later diffs are readable.

**Files:**
- Rename: `src/device.rs` → `src/usb.rs`
- Modify: `src/main.rs:5`, and every `device::` reference in `src/main.rs`, `src/controls.rs`, `src/profile.rs`, `src/tui.rs`

**Interfaces:**
- Consumes: nothing
- Produces: module path `crate::usb` exposing everything `crate::device` did — `Cam`, `Unit`, `Found`, `scan`, `fingerprint`, `NOT_RESPONDING`, `SET_CUR`, `GET_CUR`, `GET_MIN`, `GET_MAX`, `GET_RES`, `GET_DEF`, `GET_INFO`, `INFO_GET`, `INFO_SET`, `RAZER_EU1_GUID`

- [ ] **Step 1: Move the file**

```bash
git mv src/device.rs src/usb.rs
```

- [ ] **Step 2: Update the module declaration**

In `src/main.rs`, change line 5 from `mod device;` to `mod usb;`. Keep the list alphabetical:

```rust
mod controls;
mod profile;
mod service;
mod tui;
mod usb;
```

- [ ] **Step 3: Update every reference**

```bash
grep -rn "device::" src/*.rs
```

Replace `device::` with `usb::` and `crate::device::` with `crate::usb::` at each hit. Known sites: `src/main.rs:12,218,290,347`, `src/controls.rs:8`, `src/profile.rs:4,223`, `src/tui.rs:607,632`.

Do **not** rename the `--device` CLI flag (`src/main.rs:26`), the `Profile.device` field, or any user-facing string containing the word "device". Those are separate concepts — see `CONTEXT.md`.

- [ ] **Step 4: Verify**

Run: `cargo test 2>&1 | tail -20`
Expected: all tests pass, no warnings. The compiler catches any missed reference.

- [ ] **Step 5: Commit**

```bash
git add -A src/
git commit -m "refactor: rename device.rs to usb.rs

It is the USB/UVC transport layer. Leaving it as 'device' would put
usb::Cam next to models::Model under a name that means neither."
```

---

## Task 3: Vendor-neutral extension unit parsing

Extracts the descriptor byte-walk into a pure function so it can be tested without hardware, then makes it return every extension unit instead of looking for one GUID. No behaviour change: `has_razer_unit()` still answers the same question, now by looking Razer's GUID up in the list.

**Files:**
- Modify: `src/usb.rs:77-132` (`Units`, `parse_units`), `src/usb.rs:54-65` (`Cam`), `src/usb.rs:67-75` (`Found`), `src/usb.rs:175-186` (`scan`), `src/usb.rs:256-289` (`Cam::open`, `has_razer_unit`, `unit_id`)
- Test: `src/usb.rs` (new `#[cfg(test)] mod tests` at end of file)

**Interfaces:**
- Consumes: `crate::usb` from Task 2
- Produces:
  - `fn parse_extra(extra: &[u8]) -> ParsedUnits` — pure, testable
  - `pub struct ParsedUnits { pub camera_id: Option<u8>, pub processing_id: Option<u8>, pub extensions: Vec<([u8; 16], u8)> }`
  - `Cam.extension_units: Vec<([u8; 16], u8)>` (private field)
  - `Found.extension_guids: Vec<[u8; 16]>` (public field, replaces `has_razer: bool`)
  - `pub fn format_guid(bytes: &[u8; 16]) -> String` — canonical `23e49ed0-1178-4f31-ae52-d2fb8a8d3b48` form

- [ ] **Step 1: Write the failing tests**

Add at the end of `src/usb.rs`:

```rust
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
        eu.extend_from_slice(&RAZER_EU1_GUID);
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
        assert_eq!(p.extensions[0], (RAZER_EU1_GUID, 3));
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
    fn guid_renders_in_canonical_form() {
        assert_eq!(
            format_guid(&RAZER_EU1_GUID),
            "23e49ed0-1178-4f31-ae52-d2fb8a8d3b48",
            "first three fields are little-endian in the descriptor"
        );
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib usb 2>&1 | tail -20`
Expected: compile error — `cannot find function 'parse_extra'`, `cannot find function 'format_guid'`.

- [ ] **Step 3: Add the pure parser and the GUID formatter**

In `src/usb.rs`, replace the `Units` struct and `parse_units` function (lines 77-132) with:

```rust
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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib usb 2>&1 | tail -20`
Expected: 4 tests pass. The rest of the crate will not compile yet — that is Step 5.

- [ ] **Step 5: Thread the new shape through `Cam`, `Found` and `scan`**

In `src/usb.rs`, change the `Cam` struct (lines 54-65): replace `razer_id: Option<u8>` with:

```rust
    extension_units: Vec<([u8; 16], u8)>,
```

Change `Found` (lines 67-75): replace `pub has_razer: bool` with:

```rust
    /// Every extension unit GUID this camera carries, in descriptor order.
    pub extension_guids: Vec<[u8; 16]>,
```

In `scan` (lines 161-186), the loop body becomes:

```rust
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
```

Delete the `let _ = units.vc_iface;` line — `vc_iface` now lives on `Interface` and is genuinely used by `Cam::open`.

In `Cam::open`, the destructuring at line 243 becomes `let (dev, desc, iface) = chosen;` and the constructed `Cam` ends:

```rust
            vc_iface: iface.vc_iface,
            camera_id: iface.units.camera_id,
            processing_id: iface.units.processing_id,
            extension_units: iface.units.extensions,
```

Update the two `candidates` sites (lines 193-199, 223) to bind `iface` instead of `units`.

Keep `has_razer_unit` working for now, so nothing else in the crate needs touching:

```rust
    pub fn has_razer_unit(&self) -> bool {
        self.extension_units.iter().any(|(g, _)| *g == RAZER_EU1_GUID)
    }
```

Replace `unit_id`'s `Unit::Razer` arm:

```rust
            Unit::Razer => self
                .extension_units
                .iter()
                .find(|(g, _)| *g == RAZER_EU1_GUID)
                .map(|(_, id)| *id),
```

In `src/main.rs:224`, `f.has_razer` becomes `!f.extension_guids.is_empty()` — a temporary equivalence that Task 13 replaces properly.

- [ ] **Step 6: Verify the whole crate**

Run: `cargo test 2>&1 | tail -20`
Expected: all tests pass including the eight snapshots, no warnings.

- [ ] **Step 7: Commit**

```bash
git add src/usb.rs src/main.rs
git commit -m "refactor: parse every extension unit, not just Razer's

Extracts the descriptor byte-walk into a pure parse_extra() so it is
testable without hardware. No behaviour change: has_razer_unit() now
looks Razer's GUID up in the list."
```

---

## Task 4: `Kind::Razer` becomes `Kind::Opaque`, payloads widen

Mechanical rename plus one type widening. The widening is what lets a vendor with variable-length payloads (AnkerWork) contribute without editing kiyoctl's types.

**Files:**
- Modify: `src/controls.rs:10-26` (`RazerOpt`, `Kind`), `src/controls.rs:47-74` (payload consts), `src/controls.rs:142-156` (`is_razer`, `choices`), `src/controls.rs:190-283` (`read`, `write`)
- Modify: `src/main.rs`, `src/profile.rs`, `src/tui.rs` — every `is_razer()` call site
- Test: `src/controls.rs` (new `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `crate::usb` from Task 3
- Produces:
  - `pub struct OpaqueOpt { pub name: &'static str, pub payload: &'static [u8], pub pre: Option<&'static [u8]> }`
  - `Kind::Opaque(&'static [OpaqueOpt])`
  - `Control::is_opaque(&self) -> bool`

- [ ] **Step 1: Write the failing test**

Add at the end of `src/controls.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_controls_report_their_option_names() {
        let hdr = find("hdr").expect("hdr must exist");
        assert!(hdr.is_opaque());
        assert_eq!(hdr.choices(), Some(vec!["off", "on"]));
    }

    #[test]
    fn standard_controls_are_not_opaque() {
        assert!(!find("brightness").unwrap().is_opaque());
        assert!(!find("auto_exposure").unwrap().is_opaque());
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
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib controls 2>&1 | tail -20`
Expected: compile error — `no method named 'is_opaque'`, `cannot find type 'OpaqueOpt'`.

- [ ] **Step 3: Rename and widen**

In `src/controls.rs`, replace lines 10-26 with:

```rust
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
```

Rewrite the four payload tables (lines 47-74) with the new type. Every `payload: [...]` becomes `payload: &[...]` and every `pre: Some([...])` becomes `pre: Some(&[...])`:

```rust
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
```

Change `RAZER_SAVE` (line 45) to a slice:

```rust
pub const RAZER_SAVE: &[u8] = &[0xc0, 0x03, 0xa8, 0x00, 0x00, 0x00, 0x00, 0x00];
```

Rename the method (lines 143-145):

```rust
    pub fn is_opaque(&self) -> bool {
        matches!(self.kind, Kind::Opaque(_))
    }
```

Update `choices` (line 152), `read` (line 191), and `write` (line 242) to match on `Kind::Opaque` instead of `Kind::Razer`. In `write`, `cam.set(ctrl.unit, ctrl.selector, pre)` now takes `pre` directly rather than `&opt.pre` — it is already a slice reference.

Update the four `Kind::Razer(...)` uses in the `CONTROLS` table (lines 128-135) to `Kind::Opaque(...)`.

- [ ] **Step 4: Rename every call site**

```bash
grep -rn "is_razer\|RazerOpt\|Kind::Razer" src/*.rs
```

Replace `is_razer()` with `is_opaque()` at all remaining hits: `src/main.rs:237,240,260,301,311,338,368,450`, `src/profile.rs:140,184,213`, `src/tui.rs:580,606,616,630,668,736,864`. `src/main.rs:347` and `src/profile.rs:223` pass `&controls::RAZER_SAVE`, which must become `controls::RAZER_SAVE` now that it is already a slice.

Two sites need care because they mean "only exists on this camera", not "cannot be read back" — they keep `has_razer_unit()` for now and are fixed in Task 8: `src/main.rs:334` and `src/main.rs:184` in `profile.rs`. Leave their `is_opaque() && !cam.has_razer_unit()` shape as-is for this task.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test 2>&1 | tail -20`
Expected: all tests pass, all eight snapshots unchanged, no warnings.

- [ ] **Step 6: Commit**

```bash
git add src/
git commit -m "refactor: Kind::Razer becomes Kind::Opaque, payloads widen to slices

Opaque means write-only named payloads with no read-back — a fact
independent of living on an extension unit. Widening payload and pre
from [u8; 8] to &[u8] lets a vendor with variable-length buffers
contribute without editing kiyoctl's types."
```

---

## Task 5: The Model registry

The core of the SPI. Introduces `src/models/`, moves Razer's GUID and controls out of the shared catalogue, and makes `Unit::Extension` address a unit by GUID.

**Files:**
- Create: `src/models/mod.rs`
- Create: `src/models/razer_kiyo_pro.rs`
- Modify: `src/usb.rs` — `Unit` enum, `unit_id`, `Cam.model`, `Cam::open`
- Modify: `src/controls.rs` — remove the four Razer controls and their payload tables
- Modify: `src/main.rs:4-8` — add `mod models;`
- Test: `src/models/mod.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `usb::Unit`, `usb::format_guid`, `controls::{Control, Kind, OpaqueOpt}` from Tasks 3-4
- Produces:
  - `pub struct Model { pub name: &'static str, pub usb_ids: &'static [(u16, u16)], pub controls: &'static [Control], pub persist: Option<(Unit, u8, &'static [u8])> }`
  - `pub static MODELS: &[Model]`
  - `pub fn for_camera(vid: u16, pid: u16) -> Option<&'static Model>`
  - `usb::Unit::Extension(&'static [u8; 16])` replacing `Unit::Razer`
  - `Cam.model: Option<&'static Model>` (public field)
  - `models::razer_kiyo_pro::EU1_GUID`

- [ ] **Step 1: Write the failing tests**

Create `src/models/mod.rs` containing only the test module for now, so the tests exist before the code:

```rust
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
```

Add to `src/usb.rs`'s test module as well — this is the guard against writing
to the wrong unit when a camera does not carry the GUID a control names:

```rust
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib models 2>&1 | tail -20`
Expected: compile error — `cannot find value 'MODELS'`, `cannot find function 'for_camera'`.

- [ ] **Step 3: Write the registry**

Prepend to `src/models/mod.rs`, above the test module:

```rust
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
```

- [ ] **Step 4: Write the Kiyo Pro Model**

Create `src/models/razer_kiyo_pro.rs`. The payload tables move here verbatim from `src/controls.rs`:

```rust
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

pub static MODEL: Model = Model {
    name: "Razer Kiyo Pro",
    usb_ids: &[(0x1532, 0x0e05)],
    controls: CONTROLS,
    persist: Some((UNIT, EU1_SET_ISP, SAVE)),
};
```

- [ ] **Step 5: Change `Unit` to address extension units by GUID**

In `src/usb.rs`, replace the `Unit` enum (lines 37-46):

```rust
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
```

Replace `unit_id`'s body (lines 283-289). The extension lookup is a free
function so it can be tested without opening a camera:

```rust
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
```

```rust
    fn unit_id(&self, unit: Unit) -> Option<u8> {
        match unit {
            Unit::Camera => self.camera_id,
            Unit::Processing => self.processing_id,
            Unit::Extension(_) => find_unit(&self.extension_units, unit),
        }
    }
```

In `get` and `set` (lines 300, 313), replace `format!("{unit:?} unit not present on this camera")` with:

```rust
            .ok_or_else(|| format!("{} not present on this camera", unit.label()))?;
```

Delete `RAZER_EU1_GUID` (lines 30-33) and `has_razer_unit` (lines 269-271) — the GUID now lives in `src/models/razer_kiyo_pro.rs`. Add a `model` field to `Cam`:

```rust
    /// The Model covering this camera, if any. Selected by vid:pid.
    pub model: Option<&'static crate::models::Model>,
```

and fill it in `Cam::open` alongside the other fields:

```rust
            model: crate::models::for_camera(desc.vendor_id(), desc.product_id()),
```

Update `src/usb.rs`'s own test module: `RAZER_EU1_GUID` becomes `crate::models::razer_kiyo_pro::EU1_GUID`.

- [ ] **Step 6: Remove the Razer controls from the shared catalogue**

In `src/controls.rs`, delete the four `Unit::Razer` entries from `CONTROLS` (lines 127-135), the four payload tables (lines 47-74), `EU1_SET_ISP` (line 42) and `RAZER_SAVE` (line 45).

Then rename the static in place — `CONTROLS` becomes `STANDARD`. Do not retype its contents: the sixteen standard UVC entries (`brightness` through `zoom`, lines 89-125) stay exactly as they are, in exactly that order. Only the declaration line changes:

```rust
pub static STANDARD: &[Control] = &[
```

The order matters: Task 6's `kiyo_pro_sees_exactly_what_it_saw_before` test asserts it.

Leave `find` pointing at `STANDARD` for now; Task 6 splits it properly. The crate will not compile until Task 6 — that is expected and this task's tests run with `--lib models` only if the crate compiles, so complete Step 7 before running the full suite.

- [ ] **Step 7: Register the module and get back to green**

In `src/main.rs`, add `mod models;` to the module list (keep alphabetical, after `mod main`-adjacent entries):

```rust
mod controls;
mod models;
mod profile;
mod service;
mod tui;
mod usb;
```

Fix the now-broken references the compiler reports by pointing them at `STANDARD`. Every `controls::CONTROLS` becomes `controls::STANDARD` for now. The four Razer control names will be missing from `main.rs`'s and `tui.rs`'s lists until Task 6 — that is a real, temporary behaviour regression, which is why Tasks 5 and 6 land back to back.

Run: `cargo test --lib models 2>&1 | tail -20`
Expected: the seven registry tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/models/ src/usb.rs src/controls.rs src/main.rs src/tui.rs src/profile.rs
git commit -m "feat: add the Model registry, matched by vid:pid

Razer's GUID, payloads and controls move into src/models/razer_kiyo_pro.rs.
Unit::Extension addresses a unit by GUID, so one Model may span several.
Matching is by vid:pid only, which fixes misidentifying the Dell
UltraSharp WB7022 as a Kiyo Pro. See ADR 0002."
```

---

## Task 6: Split the control list three ways

Restores the four Razer controls to every place that should see them, now sourced from the attached Model rather than a global static. Test 2 here is the regression guard for the whole refactor.

**Files:**
- Modify: `src/controls.rs` — add `effective_controls`, `every`, `find_any`, `is_model_control`
- Modify: `src/usb.rs` — add `Cam::controls`, `Cam::find`
- Test: `src/controls.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `models::{Model, MODELS, for_camera}` from Task 5
- Produces:
  - `pub fn effective_controls(model: Option<&'static Model>) -> Vec<&'static Control>`
  - `pub fn every() -> Vec<&'static Control>`
  - `pub fn find_any(name: &str) -> Option<&'static Control>`
  - `pub fn is_model_control(name: &str) -> bool`
  - `Cam::controls(&self) -> Vec<&'static Control>`
  - `Cam::find(&self, name: &str) -> Option<&'static Control>`

- [ ] **Step 1: Write the failing tests**

Add to `src/controls.rs`'s test module:

```rust
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib controls 2>&1 | tail -20`
Expected: compile error — `cannot find function 'effective_controls'`.

- [ ] **Step 3: Write the three lookups**

In `src/controls.rs`, replace `find` (lines 138-140) with:

```rust
use crate::models::{Model, MODELS};

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
```

- [ ] **Step 4: Repoint the existing `find` callers**

`controls::find` no longer exists. Every current caller becomes `find_any`,
which is behaviour-preserving — `every()` is a superset of the old `CONTROLS`.
Tasks 8 and 10 then refine the camera-bound ones to `cam.find`.

```bash
grep -rn "controls::find(" src/*.rs
```

Known sites: `src/profile.rs:92` (`set`), `src/profile.rs:171` (the apply plan),
`src/profile.rs:177` (the unknown-name scan), `src/profile.rs:196` (the
prerequisite lookup), `src/main.rs:310` (`cmd_get`), `src/main.rs:333`
(`cmd_set`), `src/main.rs:605` (`unknown_control`). Replace `controls::find(`
with `controls::find_any(` at each.

- [ ] **Step 5: Add the camera-bound wrappers**

In `src/usb.rs`, inside `impl Cam`:

```rust
    /// The controls this camera has: standard UVC plus its Model's, if any.
    pub fn controls(&self) -> Vec<&'static crate::controls::Control> {
        crate::controls::effective_controls(self.model)
    }

    /// Look a control up among the ones this camera actually has.
    pub fn find(&self, name: &str) -> Option<&'static crate::controls::Control> {
        self.controls().into_iter().find(|c| c.name == name)
    }

    /// The Model's name, for user-facing text.
    pub fn model_name(&self) -> Option<&'static str> {
        self.model.map(|m| m.name)
    }
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test 2>&1 | tail -20`
Expected: 5 new tests pass, including `kiyo_pro_sees_exactly_what_it_saw_before`. The whole crate compiles again — Task 5 left it broken on purpose, and Steps 3-4 here are what close it.

- [ ] **Step 7: Commit**

```bash
git add src/controls.rs src/usb.rs
git commit -m "feat: split the control catalogue into STANDARD, per-camera and every

effective_controls() is a pure function over Option<&Model>, so the
list a camera sees is testable without hardware. The
kiyo_pro_sees_exactly_what_it_saw_before test is the regression guard
for the whole refactor."
```

---

## Task 7: One `Cam::persist()`, replacing three copies

The persist hook is written three times today, each hardcoding selector `0x01` and Razer's payload rather than reading them from the Model.

**Files:**
- Modify: `src/usb.rs` — add `Cam::persist`
- Modify: `src/profile.rs:182-224`, `src/main.rs:331-348`, `src/tui.rs:626-643`
- Test: `src/usb.rs`

**Interfaces:**
- Consumes: `Model.persist` from Task 5
- Produces: `Cam::persist(&self)` — issues the attached Model's persist triple, or does nothing

- [ ] **Step 1: Write the failing test**

Add to `src/usb.rs`'s test module:

```rust
    #[test]
    fn the_kiyo_pro_declares_a_persist_triple_on_its_own_unit() {
        let m = &crate::models::razer_kiyo_pro::MODEL;
        let (unit, selector, payload) = m.persist.expect("the Kiyo Pro persists its state");
        assert_eq!(
            unit,
            Unit::Extension(&crate::models::razer_kiyo_pro::EU1_GUID),
            "persist must target the Model's own extension unit, not a hardcoded one"
        );
        assert_eq!(selector, 0x01);
        assert_eq!(payload, &[0xc0, 0x03, 0xa8, 0x00, 0x00, 0x00, 0x00, 0x00]);
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib usb::tests::the_kiyo 2>&1 | tail -20`
Expected: FAIL — `Unit` does not implement `PartialEq` for this comparison, or the field does not exist yet if Task 5's Model was written differently. If it passes immediately, Task 5 already satisfied it; continue to Step 3.

- [ ] **Step 3: Add `Cam::persist`**

In `src/usb.rs`, inside `impl Cam`:

```rust
    /// Ask the camera to keep its extension-unit state across a power cycle.
    ///
    /// Called once per operation that wrote at least one extension-unit
    /// control — a TUI edit counts as one operation. Silent on failure: the
    /// settings are already applied, and a camera that refuses to persist is
    /// not worth failing the command over.
    pub fn persist(&self) {
        let Some(model) = self.model else { return };
        let Some((unit, selector, payload)) = model.persist else { return };
        let _ = self.set(unit, selector, payload);
    }
```

- [ ] **Step 4: Replace the three copies**

In `src/profile.rs`, replace lines 182 and 209-224. The flag becomes "touched an extension unit", not "touched an opaque control" — those are different facts now:

Rename the flag at line 182 and change what sets it. The `requires` /
prerequisite block in the middle of the loop (lines 191-207) is untouched —
leave those seventeen lines exactly as they are and edit only around them:

```rust
    let mut touched_extension = false;
    for (ctrl, value) in &planned {
        // (the existing `if let Some((dep, allowed)) = ctrl.requires` block
        //  stays here verbatim)
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

    if touched_extension {
        cam.persist();
    }
```

In `src/main.rs:331-348`, the same shape: rename `touched_razer` to `touched_extension`, set it from `matches!(ctrl.unit, usb::Unit::Extension(_))`, and replace the `cam.set(...)` call with `cam.persist();`.

In `src/tui.rs:626-643`, replace the body of the `Ok(())` arm:

```rust
            Ok(()) => {
                if ctrl.is_opaque() {
                    self.ui.rows[index].value = Some(value.clone());
                } else {
                    self.refresh_values();
                }
                if matches!(ctrl.unit, crate::usb::Unit::Extension(_)) {
                    // A TUI edit is one operation, like one CLI invocation.
                    self.cam.persist();
                }
                self.profile.set(ctrl.name, &value);
                self.ui.dirty = true;
                self.ui.status = format!("{} = {}", ctrl.name, value);
            }
```

Note the reordering: the row's displayed value is set from `is_opaque()` (cannot be read back) while persist is keyed on `Unit::Extension` (lives in the camera's own storage). Conflating them is the bug this refactor exists to fix.

- [ ] **Step 5: Verify**

Run: `cargo test 2>&1 | tail -20`
Expected: all tests pass, all eight snapshots unchanged, no warnings.

- [ ] **Step 6: Commit**

```bash
git add src/usb.rs src/profile.rs src/main.rs src/tui.rs
git commit -m "refactor: one Cam::persist(), replacing three hardcoded copies

Each copy hardcoded selector 0x01 and Razer's payload. It now reads the
attached Model's persist triple, and is keyed on writing an extension
unit rather than on writing an opaque control — two different facts."
```

---

## Task 8: Rewire `main.rs`

Splits the remaining "only exists on this camera" sites onto the Model, and substitutes the Model name into user-facing text. This is where the Dell fix becomes visible.

**Files:**
- Modify: `src/main.rs:217-231` (`cmd_list`), `233-252` (`cmd_controls`), `254-307` (`cmd_show`), `309-324` (`cmd_get`), `326-356` (`cmd_set`), `358-381` (`cmd_save`), `446-468` (`cmd_reset`), `604-607` (`unknown_control`)

**Interfaces:**
- Consumes: `Cam::controls`, `Cam::find`, `Cam::model_name`, `controls::{every, find_any}` from Task 6
- Produces: nothing consumed by later tasks

- [ ] **Step 1: `cmd_controls` — group by Model**

Replace the body of `cmd_controls` (lines 233-252):

```rust
fn cmd_controls() -> Result<(), String> {
    let all = controls::every();
    let width = all.iter().map(|c| c.name.len()).max().unwrap_or(0);

    let describe = |ctrl: &controls::Control| {
        let values = match ctrl.choices() {
            Some(c) => c.join(" | "),
            None => "a number (range depends on the camera)".to_string(),
        };
        println!("  {:<width$}  {}\n  {:<width$}  values: {values}", ctrl.name, ctrl.help, "");
        println!();
    };

    for ctrl in controls::STANDARD {
        describe(ctrl);
    }
    for model in models::MODELS {
        println!("  -- {} --\n", model.name);
        for ctrl in model.controls {
            describe(ctrl);
        }
    }
    println!("Not every camera implements every control; `kiyoctl show` lists what yours does.");
    Ok(())
}
```

- [ ] **Step 2: `cmd_show` — use the camera's own list, name the Model**

In `cmd_show`, replace `for ctrl in CONTROLS` (line 259) with `for ctrl in cam.controls()`, and replace the trailing Razer block (lines 299-305):

```rust
    if let Some(model) = cam.model {
        let opaque: Vec<&controls::Control> =
            model.controls.iter().filter(|c| c.is_opaque()).collect();
        if !opaque.is_empty() {
            println!(
                "\n{} (write-only — the camera does not report these back):",
                model.name
            );
            for ctrl in opaque {
                let choices = ctrl.choices().unwrap_or_default().join("|");
                println!("  {:<width$}  {:>vwidth$}   {choices}", ctrl.name, "?");
            }
        }
    }
```

- [ ] **Step 3: `cmd_get`, `cmd_set`, `cmd_reset` — availability against the Model**

In `cmd_get` (line 310), `controls::find(name)` becomes `controls::find_any(name)`. Keep the `is_opaque()` guard.

In `cmd_set` (lines 333-336), replace the lookup and the availability check:

```rust
        let ctrl = cam
            .find(name)
            .ok_or_else(|| unavailable_control(&cam, name))?;
```

and delete the `if ctrl.is_razer() && !cam.has_razer_unit()` block entirely — a control absent from `cam.find` is already unavailable.

In `cmd_reset` (line 449), `for ctrl in CONTROLS` becomes `for ctrl in cam.controls()`, and the trailing Razer note (lines 464-466) becomes:

```rust
    if let Some(model) = cam.model {
        if model.controls.iter().any(|c| c.is_opaque()) {
            println!(
                "{} settings were left alone — set them explicitly if needed.",
                model.name
            );
        }
    }
```

- [ ] **Step 4: `cmd_save` — name the Model in the note**

Replace lines 365-379:

```rust
    if let Some(model) = cam.model {
        let tracked: Vec<&str> = model
            .controls
            .iter()
            .filter(|c| c.is_opaque() && prof.controls.contains_key(c.name))
            .map(|c| c.name)
            .collect();
        if tracked.is_empty() {
            println!(
                "Note: {} settings cannot be read back. Set them once with \
                 `kiyoctl set hdr on` and they will be remembered from then on.",
                model.name
            );
        } else {
            println!("Remembered {} settings: {}", model.name, tracked.join(", "));
        }
    }
```

- [ ] **Step 5: Distinguish "not on this camera" from "unknown control"**

Replace `unknown_control` (lines 604-607) with two functions:

```rust
fn unknown_control(name: &str) -> String {
    let known: Vec<&str> = controls::every().iter().map(|c| c.name).collect();
    format!("unknown control '{name}'. Known controls: {}", known.join(", "))
}

/// A name kiyoctl knows but this camera does not have reads very differently
/// from a name nobody has ever heard of.
fn unavailable_control(cam: &Cam, name: &str) -> String {
    if controls::find_any(name).is_some() {
        format!("{} does not have {name}", cam.name)
    } else {
        unknown_control(name)
    }
}
```

- [ ] **Step 6: Fix `cmd_list`'s temporary shim**

Restore the Model name in place of Task 3's placeholder (line 224):

```rust
        let extra = match models::for_camera(f.vid, f.pid) {
            Some(m) => format!("  [{}]", m.name),
            None => String::new(),
        };
```

Task 13 adds the unrecognised-unit marker here.

- [ ] **Step 7: Verify by hand as well as by test**

Run: `cargo test 2>&1 | tail -20`
Expected: all tests pass, no warnings.

Then, with a Kiyo Pro attached, confirm nothing regressed:

```bash
cargo run -- list
cargo run -- list-controls | head -40
cargo run -- show
```

Expected: `list` shows `[Razer Kiyo Pro]`; `list-controls` shows the standard controls then a `-- Razer Kiyo Pro --` group with `hdr`, `hdr_mode`, `fov`, `af_mode`; `show` lists readable values then the write-only block headed `Razer Kiyo Pro`.

With no Kiyo Pro attached, `cargo run -- list-controls` must still show the `-- Razer Kiyo Pro --` group, because `every()` does not depend on hardware.

- [ ] **Step 8: Commit**

```bash
git add src/main.rs
git commit -m "feat: main.rs reads controls from the attached Model

Availability now means 'this Model declares it' rather than 'the camera
has a Razer unit', and user-facing text substitutes the Model name.
A Dell UltraSharp WB7022 stops being offered Razer controls."
```

---

## Task 9: Rewire `tui.rs`

The snapshots are the contract: eight of them must not move.

**Files:**
- Modify: `src/tui.rs:88-107` (`Ui`), `553-611` (`App::new`, `reload`), `660-684` (`apply_profile`), `936-940` (the note), `1213-1240` (the `ui()` fixture)

**Interfaces:**
- Consumes: `Cam::controls`, `Cam::model_name` from Task 6
- Produces: `Ui.model_name: Option<String>` replacing `Ui.has_razer: bool`

- [ ] **Step 1: Replace the `Ui` field**

In `src/tui.rs`, replace line 93:

```rust
    /// The attached camera's Model, if kiyoctl has one for it.
    model_name: Option<String>,
```

- [ ] **Step 2: Fill it from the camera**

In `App::new` (line 560), replace `has_razer: cam.has_razer_unit(),`:

```rust
            model_name: cam.model_name().map(str::to_string),
```

- [ ] **Step 3: Drive `reload` from the camera's own control list**

Replace `reload`'s loop head (line 579) and the opaque branch (lines 580-593):

```rust
        for ctrl in self.cam.controls() {
            if ctrl.is_opaque() {
                // Write-only: the best we can show is what the profile recalls.
                rows.push(Row {
                    ctrl,
                    value: self.profile.get(ctrl.name),
                    range: None,
                    step: 1,
                    default: None,
                    writable: true,
                });
                continue;
            }
```

The `if self.cam.has_razer_unit()` guard disappears: `cam.controls()` only yields a Model's controls when that Model is attached, so the check is already made.

Replace line 606's guard:

```rust
        if !rows.iter().any(|r| !r.ctrl.is_opaque()) && !self.cam.responding {
            return Err(crate::usb::NOT_RESPONDING.into());
        }
```

Replace `is_razer()` with `is_opaque()` at lines 616, 668, 736, 864 — all four mean "cannot be read back", which is exactly `is_opaque`.

- [ ] **Step 4: Re-key the panel note**

Replace lines 936-940. The note is about opaque rows being present, not about a vendor:

```rust
    let opaque_note = if ui.rows.iter().any(|r| r.ctrl.is_opaque()) {
        " (magenta = write-only, remembered by kiyoctl) "
    } else {
        " "
    };
```

and use `{opaque_note}` in the title format at line 945. The rendered text is unchanged, which is why the snapshots hold.

- [ ] **Step 5: Update the test fixture**

In the `ui()` fixture (line 1218), replace `has_razer: true,`:

```rust
            model_name: Some("Razer Kiyo Pro".into()),
```

- [ ] **Step 6: Verify the snapshots did not move**

Run: `cargo test 2>&1 | tail -30`
Expected: all tests pass. **If any snapshot fails, stop.** Do not run `cargo insta accept`. A moved snapshot means the rendering changed, which this task must not do — report the diff instead.

- [ ] **Step 7: Delete the last of the vendor-specific transport**

One caller of `has_razer_unit` survives outside the TUI: `src/profile.rs:184`,
the `if ctrl.is_opaque() && !cam.has_razer_unit()` availability check in
`apply`. Task 10 rewrites `apply` entirely, but `has_razer_unit` must go now,
so replace that block with the Model-based check it will keep:

```rust
        if cam.find(ctrl.name).is_none() {
            report
                .skipped
                .push((ctrl.name.into(), "not available on this camera".into()));
            continue;
        }
```

Then:

```bash
grep -rn "has_razer\|is_razer\|Unit::Razer\|RAZER" src/*.rs src/models/*.rs
```

Expected: hits only inside `src/models/razer_kiyo_pro.rs` and in comments naming the camera. Delete `Cam::has_razer_unit` from `src/usb.rs` — nothing calls it now. If the grep still shows a caller, fix that instead of leaving the method.

Run: `cargo test 2>&1 | tail -20`
Expected: all tests pass, no warnings, no dead-code warnings.

- [ ] **Step 8: Commit**

```bash
git add src/tui.rs src/usb.rs
git commit -m "feat: TUI reads controls from the attached Model

Ui.has_razer becomes Ui.model_name; the magenta note is re-keyed from
'camera has a Razer unit' to 'any opaque row is present'. Rendered text
is unchanged and all eight snapshots hold."
```

---

## Task 10: Per-camera profile sections, with migration

The migration is the load-bearing part: without it, every existing user's HDR silently stops applying. See ADR 0003.

**Files:**
- Modify: `src/profile.rs:10-18` (`Profile`), `60-126` (`load`, `save`, `set`, `get`), `137-153` (`capture`), `165-227` (`apply`)
- Modify: `src/main.rs:351-352` (`cmd_set`), `397-418` (`cmd_profiles`)
- Test: `src/profile.rs` (new `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `controls::is_model_control` from Task 6
- Produces:
  - `Profile.camera: Option<String>` (was `device`, with `#[serde(alias = "device")]`)
  - `Profile.per_camera: BTreeMap<String, BTreeMap<String, Json>>`
  - `Profile::migrate(&mut self)`
  - `Profile::key(vid: u16, pid: u16) -> String` — the `"vvvv:pppp"` section key
  - `Profile::values_for(&self, vid: u16, pid: u16) -> BTreeMap<String, Json>`

- [ ] **Step 1: Write the failing tests**

Add at the end of `src/profile.rs`:

```rust
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib profile 2>&1 | tail -20`
Expected: compile error — `no field 'camera'`, `no method 'migrate'`, `no method 'values_for'`.

- [ ] **Step 3: Change the struct**

In `src/profile.rs`, replace lines 10-18:

```rust
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
```

- [ ] **Step 4: Write the key, the migration and the lookup**

Add to `impl Profile`:

```rust
    /// The section key for a camera.
    pub fn key(vid: u16, pid: u16) -> String {
        format!("{vid:04x}:{pid:04x}")
    }

    /// Move Model control values out of the flat map and into the section for
    /// the camera this profile records.
    ///
    /// Profiles written by 0.2.0 keep `hdr` and `fov` in the flat map. Applying
    /// that map as standard-controls-only would silently stop applying them, so
    /// this runs on every load. Idempotent. A profile with no recorded camera
    /// keeps its values flat, because guessing which camera they came from is
    /// worse than leaving them where they are.
    pub fn migrate(&mut self) {
        let Some(camera) = self.camera.clone() else { return };
        let moving: Vec<String> = self
            .controls
            .keys()
            .filter(|name| controls::is_model_control(name))
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
```

Call it from `load` (line 61):

```rust
    pub fn load(name: &str) -> Result<Profile, String> {
        let path = profile_path(name);
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read profile '{name}' ({}): {e}", path.display()))?;
        let mut profile: Profile = serde_json::from_str(&text)
            .map_err(|e| format!("profile '{name}' is not valid JSON: {e}"))?;
        profile.migrate();
        Ok(profile)
    }
```

`load_or_default` already delegates to `load`, so it migrates too.

- [ ] **Step 5: Route writes into the right map**

Replace `Profile::set` (lines 91-100) so a Model control lands in its camera's section:

```rust
    /// Record one control value, preserving natural JSON types.
    ///
    /// `camera` must be set first for a Model control to reach its section;
    /// `set_for` is the explicit form used by capture and the CLI.
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
```

Replace `Profile::get` (lines 103-105) so the TUI's remembered-value lookup sees both maps:

```rust
    /// The stored value rendered the way the CLI accepts it. Searches the flat
    /// map and every camera section, most recent camera first.
    pub fn get(&self, control: &str) -> Option<String> {
        if let Some(v) = self.controls.get(control) {
            return Some(render(v));
        }
        let mine = self.camera.as_ref().and_then(|c| self.per_camera.get(c));
        mine.and_then(|s| s.get(control)).map(render)
    }
```

- [ ] **Step 6: Apply and capture use the merged view**

In `apply` (line 168), build the plan from `values_for`:

```rust
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
```

The rest of `apply` is unchanged except that `profile.get(dep)` in the prerequisite lookup (line 194) becomes `values.get(dep).map(render)`.

In `capture` (line 139), iterate the camera's own list and set the identity *before* recording values, so `set` routes them correctly:

```rust
pub fn capture(cam: &Cam, profile: &mut Profile) -> Vec<String> {
    // Identity first: `set` uses it to decide which map a Model control
    // belongs in. Any previous camera's values are already in their own
    // section, so re-homing loses nothing.
    profile.camera = Some(Profile::key(cam.vid, cam.pid));
    profile.name = Some(cam.name.clone());

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
```

- [ ] **Step 7: Update the two `main.rs` callers**

In `cmd_set` (lines 351-352), set the identity before the loop rather than after, for the same reason:

```rust
    let cam = Cam::open(dev)?;
    let mut prof = if no_remember { Profile::default() } else { Profile::load_or_default(profile_name)? };
    if !no_remember {
        prof.camera = Some(Profile::key(cam.vid, cam.pid));
        prof.name = Some(cam.name.clone());
    }
```

and delete the two assignments that were after the loop.

In `cmd_profiles` (lines 405-415), count and list both maps:

```rust
    for name in names {
        let prof = Profile::load(&name)?;
        let camera = prof.name.clone().unwrap_or_else(|| "unknown camera".into());
        let mark = if name == active { " * in use" } else { "" };
        let extra: usize = prof.per_camera.values().map(|s| s.len()).sum();
        println!("{name}  ({camera}, {} settings){mark}", prof.controls.len() + extra);
        for (k, v) in &prof.controls {
            println!("    {k} = {}", render(v));
        }
        for (cam_key, section) in &prof.per_camera {
            for (k, v) in section {
                println!("    {k} = {} [{cam_key}]", render(v));
            }
        }
    }
```

`render` is private to `profile.rs`; make it `pub(crate) fn render` so `main.rs` can use it, and delete `main.rs`'s inline copy of the same match.

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cargo test 2>&1 | tail -30`
Expected: 8 new profile tests pass, all eight snapshots unchanged, no warnings.

- [ ] **Step 9: Verify the migration against a real 0.2.0 profile**

```bash
cp -r ~/.config/kiyoctl/profiles /tmp/kiyoctl-profiles-backup
cat ~/.config/kiyoctl/profiles/default.json
cargo run -- apply
cat ~/.config/kiyoctl/profiles/default.json
```

Expected: the first `cat` shows `device` and a flat map containing `hdr`; `apply` reports the same settings applied as before the upgrade, including `hdr`; the second `cat` is unchanged, because `apply` does not save. Then:

```bash
cargo run -- save
cat ~/.config/kiyoctl/profiles/default.json
```

Expected: the file now has `camera` and a `per_camera` section holding `hdr`, and no `hdr` in the flat map. Restore from `/tmp/kiyoctl-profiles-backup` if anything looks wrong.

- [ ] **Step 10: Commit**

```bash
git add src/profile.rs src/main.rs
git commit -m "feat: profiles hold Model values in per-camera sections

Opaque values cannot be read back from hardware, so a flat profile lost
them permanently on the first capture from a different camera. Existing
profiles migrate on load, keyed by the camera they already record.
See ADR 0003."
```

---

## Task 11: Warn when a profile crosses cameras

**Files:**
- Modify: `src/profile.rs` — add `ApplyReport.captured_from`
- Modify: `src/main.rs:383-395` (`cmd_apply`), `420-444` (`cmd_use`)
- Test: `src/profile.rs`

**Interfaces:**
- Consumes: `Profile.camera`, `Profile::key` from Task 10
- Produces: `ApplyReport.captured_from: Option<String>` — `Some(key)` when the profile names a different camera than the one applied to

- [ ] **Step 1: Write the failing test**

Add to `src/profile.rs`'s test module:

```rust
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
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib profile::tests::a_profile_from 2>&1 | tail -20`
Expected: FAIL — `no method named 'foreign_to'`.

- [ ] **Step 3: Implement it**

Add to `impl Profile`:

```rust
    /// The camera this profile was captured from, when that is not the camera
    /// being applied to. Informational: cross-camera profiles are legitimate,
    /// since standard UVC values transfer.
    pub fn foreign_to(&self, vid: u16, pid: u16) -> Option<String> {
        let recorded = self.camera.clone()?;
        (recorded != Profile::key(vid, pid)).then_some(recorded)
    }
```

- [ ] **Step 4: Report it**

In `cmd_apply` (after line 386), before printing the applied lines:

```rust
    if let Some(from) = prof.foreign_to(cam.vid, cam.pid) {
        let was = prof.name.as_deref().unwrap_or("another camera");
        eprintln!(
            "this profile was captured from {was} ({from}); applying to {} ({:04x}:{:04x})",
            cam.name, cam.vid, cam.pid
        );
    }
```

Add the same block to `cmd_use` (after line 435, inside the `Ok(cam)` arm).

- [ ] **Step 5: Verify**

Run: `cargo test 2>&1 | tail -20`
Expected: all tests pass, no warnings.

- [ ] **Step 6: Commit**

```bash
git add src/profile.rs src/main.rs
git commit -m "feat: say so when a profile is applied to a different camera

Informational, not blocking — standard UVC values transfer fine, and
the skipped list already explains the rest."
```

---

## Task 12: `kiyoctl probe`

Read-only. The output is the evidence a contributor pastes into an issue, so it must be complete and paste-able.

**Files:**
- Create: `src/probe.rs`
- Modify: `src/usb.rs` — add `GET_LEN`, `Cam::extension_unit_guids`, make `get` usable for the walk
- Modify: `src/main.rs` — `mod probe;`, the `Probe` subcommand, `cmd_probe`
- Test: `src/probe.rs`

**Interfaces:**
- Consumes: `Cam`, `usb::format_guid` from Task 3
- Produces:
  - `pub const GET_LEN: u8 = 0x85;` in `src/usb.rs`
  - `Cam::extension_unit_guids(&self) -> Vec<[u8; 16]>`
  - `pub struct Selector { pub selector: u8, pub info: u8, pub len: Option<u16>, pub current: Option<Vec<u8>> }`
  - `pub fn report(guid: &[u8; 16], vid: u16, pid: u16, found: &[Selector]) -> String`
  - `pub fn run(cam: &Cam) -> String`

- [ ] **Step 1: Write the failing test**

Create `src/probe.rs` with the test module first:

```rust
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
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib probe 2>&1 | tail -20`
Expected: compile error — `cannot find type 'Selector'`.

- [ ] **Step 3: Write the report formatter**

Prepend to `src/probe.rs`:

```rust
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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib probe 2>&1 | tail -20`
Expected: 3 tests pass.

- [ ] **Step 5: Add `GET_LEN` and the unit list to the transport**

In `src/usb.rs`, add beside the other request codes (after line 15):

```rust
pub const GET_LEN: u8 = 0x85;
```

and inside `impl Cam`:

```rust
    /// Every extension unit GUID this camera carries, in descriptor order.
    pub fn extension_unit_guids(&self) -> Vec<[u8; 16]> {
        self.extension_units.iter().map(|(g, _)| *g).collect()
    }
```

- [ ] **Step 6: Write the walk**

Append to `src/probe.rs`:

```rust
/// Walk every selector on every extension unit. Read-only: GET_INFO, GET_LEN
/// and GET_CUR only, never SET_CUR.
pub fn run(cam: &Cam) -> String {
    let mut out = String::new();
    let guids = cam.extension_unit_guids();
    if guids.is_empty() {
        return format!("{} has no extension units.\n", cam.name);
    }
    for guid in guids {
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
```

`Box::leak` is how a runtime-discovered GUID reaches a `Unit::Extension(&'static ...)`. It leaks 16 bytes per unit, once, in a command that exits immediately — acceptable here and nowhere else.

- [ ] **Step 7: Wire up the subcommand**

In `src/main.rs`, add `mod probe;` to the module list, and a variant to `enum Cmd` after `Controls`:

```rust
    /// Report what is behind a camera's vendor extension units (read-only)
    Probe,
```

Add the dispatch arm in `run`:

```rust
        Cmd::Probe => cmd_probe(dev),
```

and the command body beside `cmd_show`:

```rust
fn cmd_probe(dev: Option<&str>) -> Result<(), String> {
    let cam = Cam::open(dev)?;
    println!("{} ({:04x}:{:04x})\n", cam.name, cam.vid, cam.pid);
    print!("{}", probe::run(&cam));
    Ok(())
}
```

- [ ] **Step 8: Verify**

Run: `cargo test 2>&1 | tail -20`
Expected: all tests pass, no warnings.

With a Kiyo Pro attached:

```bash
cargo run -- probe
```

Expected: one `extension unit 23e49ed0-1178-4f31-ae52-d2fb8a8d3b48 on 1532:0e05` block. Selector `0x01` should appear. The camera must still work afterwards — `cargo run -- show` must return values, not `NOT_RESPONDING`. If the camera wedges, the walk is too aggressive: reduce the range and report it.

- [ ] **Step 9: Commit**

```bash
git add src/probe.rs src/usb.rs src/main.rs
git commit -m "feat: add 'kiyoctl probe', read-only extension unit reconnaissance

Reports which selectors exist, their capability bits, and their current
bytes. This is the evidence a contribution is built from; it cannot say
what any of it means."
```

---

## Task 13: Report unrecognised extension units

The discovery path. Without it a contributor never learns their camera has anything behind it.

**Files:**
- Modify: `src/usb.rs` — add `Cam::unclaimed_units`
- Modify: `src/main.rs:217-231` (`cmd_list`), `254-307` (`cmd_show`)
- Modify: `src/tui.rs` — the help overlay and the `Ui` fixture
- Test: `src/usb.rs`

**Interfaces:**
- Consumes: `Cam.model`, `usb::format_guid`, `models::for_camera` from Tasks 3-5
- Produces:
  - `pub fn unclaimed(model: Option<&Model>, present: &[[u8; 16]]) -> Vec<[u8; 16]>` — pure
  - `Cam::unclaimed_units(&self) -> Vec<[u8; 16]>`
  - `Ui.unclaimed: Vec<String>` — pre-formatted GUIDs for the help overlay

- [ ] **Step 1: Write the failing test**

Add to `src/usb.rs`'s test module:

```rust
    #[test]
    fn a_units_claimed_by_the_attached_model_is_not_unrecognised() {
        let guid = crate::models::razer_kiyo_pro::EU1_GUID;
        let got = unclaimed(Some(&crate::models::razer_kiyo_pro::MODEL), &[guid]);
        assert!(got.is_empty(), "the Model's own unit is claimed");
    }

    #[test]
    fn a_unit_no_model_claims_is_reported() {
        let other = [0xaa; 16];
        let got = unclaimed(Some(&crate::models::razer_kiyo_pro::MODEL), &[other]);
        assert_eq!(got, vec![other]);
    }

    #[test]
    fn with_no_model_every_unit_is_unrecognised() {
        let a = [0xaa; 16];
        let b = [0xbb; 16];
        assert_eq!(unclaimed(None, &[a, b]), vec![a, b]);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib usb 2>&1 | tail -20`
Expected: compile error — `cannot find function 'unclaimed'`.

- [ ] **Step 3: Implement it**

In `src/usb.rs`:

```rust
/// The extension units present on a camera that no attached Model addresses.
/// Pure, so it is testable without hardware.
pub fn unclaimed(
    model: Option<&crate::models::Model>,
    present: &[[u8; 16]],
) -> Vec<[u8; 16]> {
    present
        .iter()
        .filter(|guid| {
            !model.is_some_and(|m| {
                m.controls
                    .iter()
                    .any(|c| matches!(c.unit, Unit::Extension(g) if *g == **guid))
            })
        })
        .copied()
        .collect()
}
```

and inside `impl Cam`:

```rust
    /// Extension units on this camera that its Model does not address — the
    /// signal that contributed support is possible.
    pub fn unclaimed_units(&self) -> Vec<[u8; 16]> {
        unclaimed(self.model, &self.extension_unit_guids())
    }
```

- [ ] **Step 4: Mark them in `cmd_list`**

Replace the marker built in Task 8 Step 6:

```rust
    for f in &found {
        let extra = match models::for_camera(f.vid, f.pid) {
            Some(m) => format!("  [{}]", m.name),
            None if !f.extension_guids.is_empty() => "  [unrecognised extension unit]".to_string(),
            None => String::new(),
        };
        println!(
            "{}  {:04x}:{:04x}  bus {} addr {}{}",
            f.name, f.vid, f.pid, f.bus, f.address, extra
        );
    }
```

- [ ] **Step 5: Give the actionable detail in `cmd_show`**

At the end of `cmd_show`, after the Model block:

```rust
    let unclaimed = cam.unclaimed_units();
    if !unclaimed.is_empty() {
        println!(
            "\nThis camera has {} extension unit{} kiyoctl does not recognise:",
            unclaimed.len(),
            if unclaimed.len() == 1 { "" } else { "s" }
        );
        for guid in &unclaimed {
            println!("  {}  on {:04x}:{:04x}", usb::format_guid(guid), cam.vid, cam.pid);
        }
        println!(
            "\nVendor-specific controls may exist behind {}.\n\
             Run `kiyoctl probe` to see what answers, then see docs/adding-a-camera.md",
            if unclaimed.len() == 1 { "it" } else { "them" }
        );
    }
```

- [ ] **Step 6: Add the help-overlay line**

In `src/tui.rs`, add to the `Ui` struct beside `model_name`:

```rust
    /// Pre-formatted GUIDs of extension units no Model claims.
    unclaimed: Vec<String>,
```

Fill it in `App::new`:

```rust
            unclaimed: cam
                .unclaimed_units()
                .iter()
                .map(crate::usb::format_guid)
                .collect(),
```

In `render_help` (dispatched at line 1007), append after the existing key list — only when there is something to say, so the `help_overlay` snapshots do not move:

```rust
    if !ui.unclaimed.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(format!(
            "This camera has {} unrecognised extension unit(s):",
            ui.unclaimed.len()
        )));
        for guid in &ui.unclaimed {
            lines.push(Line::from(format!("  {guid}")));
        }
        lines.push(Line::from("Run `kiyoctl probe`, then see docs/adding-a-camera.md"));
    }
```

`render_help` currently takes only a frame and an area. Change its signature to `fn render_help(frame: &mut Frame, area: Rect, ui: &Ui)` and update the call site at line 1007 to pass `ui`.

Add `unclaimed: Vec::new(),` to the `ui()` test fixture, which keeps all eight snapshots byte-identical.

- [ ] **Step 7: Verify the snapshots did not move**

Run: `cargo test 2>&1 | tail -30`
Expected: all tests pass. **If a snapshot fails, stop** — the fixture's `unclaimed` is empty, so the help overlay must render exactly as before.

With a non-Kiyo camera attached:

```bash
cargo run -- list
cargo run -- --device <that camera> show
```

Expected: `list` shows `[unrecognised extension unit]` if it has one; `show` prints the GUID block and the pointer to the docs.

- [ ] **Step 8: Commit**

```bash
git add src/usb.rs src/main.rs src/tui.rs
git commit -m "feat: report extension units no Model recognises

A camera with vendor controls kiyoctl cannot drive used to look
identical to one with none. list marks it, show gives the GUID and
points at the docs, the TUI help overlay mentions it."
```

---

## Task 14: The contributor path

The documentation is the deliverable that makes the whole SPI usable. Its most important job is stopping an agent from inventing payload bytes.

**Files:**
- Create: `docs/adding-a-camera.md`
- Create: `src/models/_template.rs`
- Modify: `README.md` — add a section, adjust the Razer-specific prose
- Modify: `src/models/mod.rs` — exclude the template from the build

**Interfaces:**
- Consumes: everything above
- Produces: nothing consumed by later tasks

- [ ] **Step 1: Write the template**

Create `src/models/_template.rs`. The leading underscore keeps it out of `mod` declarations; it is a file to copy, not to compile.

```rust
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
```

- [ ] **Step 2: Keep the template out of the build**

In `src/models/mod.rs`, the module list declares only real cameras. Add a comment above `pub mod razer_kiyo_pro;` so nobody adds the template by reflex:

```rust
// One line per camera. `_template.rs` is deliberately absent: it is a file to
// copy, not to compile.
pub mod razer_kiyo_pro;
```

- [ ] **Step 3: Write the contributor document**

Create `docs/adding-a-camera.md`:

````markdown
# Adding a camera

kiyoctl drives standard UVC controls on any webcam. Vendor-specific controls —
HDR, field of view, LED behaviour, pan and tilt — live behind a *extension
unit*, and every vendor's is different and undocumented.

Adding support means writing one file. Obtaining what goes in it is the hard
part, and this document is mostly about that.

## Do not guess payload bytes

Every test in this repository passes on invented payloads. A file full of
plausible-looking bytes compiles, passes the registry self-check, and writes
nonsense to somebody's camera.

If you do not have real bytes from one of the sources below, **stop** and open
an issue with your `kiyoctl probe` output. That is a genuinely useful
contribution; a confabulated Model is worse than nothing.

This applies with particular force if you are an agent working on someone's
behalf. You cannot capture USB traffic. Do not infer payloads from a control's
name, from another vendor's payloads, or from what would be reasonable.

## Step 1: See what is there

```
kiyoctl probe
```

This walks every selector on every extension unit and reports which exist,
their capability bits, their data length, and their current value where the
camera will admit to one. It writes nothing.

Two outcomes:

- **Selectors answer `GET_CUR`.** You are most of the way there. Change one
  setting in the vendor's own application, run `probe` again, and diff. The
  selector whose value moved is the control, and its values are readable, so
  you can use `Kind::Int`, `Bool` or `Menu` and get read-back for free.
- **Everything is write-only.** You need a USB capture. See step 2.

## Step 2: Where real payloads come from

In rough order of effort:

1. **Someone already decoded it.** Check
   [cameractrls](https://github.com/soyersoyer/cameractrls) — it covers
   Logitech, Dell UltraSharp and AnkerWork as well as the Kiyo Pro. It is
   LGPL-3.0; kiyoctl is GPL-3.0-or-later so that material can be used here,
   with the source named in your file's provenance comment.
2. **Capture the vendor's application.** Run the vendor's own control panel
   under a USB analyser — Wireshark with usbmon on Linux, or a hardware
   analyser — toggle one setting, and read off the `SET_CUR` payload.
3. **Bisect by hand.** Only if you have a camera you are willing to lose.

## Step 3: Write the file

Copy `src/models/_template.rs` to `src/models/<your_camera>.rs`, fill it in,
and add one line to `MODELS` in `src/models/mod.rs`.

Watch for three things the template calls out:

- **GUID byte order.** `probe` prints the published form; the array wants
  descriptor order, with the first three fields byte-reversed.
- **`usb_ids` is required.** Matching is by vid:pid, never by GUID, because
  GUIDs collide across vendors — the Dell UltraSharp WB7022 carries the Kiyo
  Pro's GUID and takes entirely different payloads on it.
- **`Kind::Opaque` is for controls that cannot be read back**, not for
  controls that live on an extension unit. If yours answers `GET_CUR`, use
  `Int`, `Bool` or `Menu` and the ordinary read path works unmodified.

## Step 4: Check it

```
cargo test
cargo run -- list          # your camera should show its Model name
cargo run -- show          # your controls should appear
cargo run -- set my_control on
```

The registry self-check will tell you about a missing `usb_ids`, a duplicate
vid:pid, a repeated control name, or a name that shadows a standard UVC
control.

## Step 5: Open a pull request

It needs two things:

- Your `kiyoctl probe` output, which shows the camera and the unit are real.
- A provenance line at the top of the Model file naming where the payloads
  came from — an upstream project and its licence, or your own capture.

Neither is bureaucracy. The maintainer does not own your camera and cannot
verify the bytes; provenance is what makes the file reviewable at all.
````

- [ ] **Step 4: Update the README**

In `README.md`, adjust the opening paragraph (lines 3-11) so it describes the
registry rather than implying the Kiyo Pro is the only possibility:

```markdown
Command-line and terminal-UI control of UVC webcams on macOS, with saved
profiles that are reapplied at login and whenever the camera is reconnected.
Brightness, white balance, exposure, focus and zoom are standard UVC and work
on any camera — macOS gives you no way to touch those otherwise.

Vendor-specific settings live behind a camera's extension unit and differ by
manufacturer. kiyoctl ships support for the **Razer Kiyo Pro** (HDR, HDR mode,
field of view, autofocus responsiveness). Other cameras can be added by writing
one file — see [docs/adding-a-camera.md](docs/adding-a-camera.md). If yours has
an extension unit kiyoctl does not recognise, `kiyoctl show` will say so.
```

Add a `## Adding a camera` section before `## Credit`, pointing at the doc, and
add `probe` to the command list wherever `list-controls` is documented.

- [ ] **Step 5: Verify**

Run: `cargo test 2>&1 | tail -20`
Expected: all tests pass, no warnings. `_template.rs` is not compiled, so a
deliberate error in it would not be caught — confirm it is genuinely excluded:

```bash
cargo build 2>&1 | grep -c template
```

Expected: `0`.

Check every relative link in the new document resolves:

```bash
grep -o "(docs/[^)]*)\|(\.\./[^)]*)" docs/adding-a-camera.md README.md
ls docs/adr/0001-relicense-to-gpl-3.md docs/adr/0002-match-models-by-usb-id.md docs/adding-a-camera.md
```

- [ ] **Step 6: Commit**

```bash
git add docs/adding-a-camera.md src/models/_template.rs src/models/mod.rs README.md
git commit -m "docs: how to add a camera, and a template to copy

The template and the document both open with 'do not guess payload
bytes', because every test in this repository passes on invented ones.
A PR needs probe output and a provenance line."
```

---

## Final verification

- [ ] **Full suite, no warnings**

```bash
cargo clean && cargo build 2>&1 | tail -5 && cargo test 2>&1 | tail -20
```

- [ ] **No vendor names left outside the Model file**

```bash
grep -rn "razer\|Razer\|RAZER" src/*.rs | grep -v "^src/models/"
```

Expected: only comments naming the camera as an example, and the Dell-collision
note in `src/usb.rs`. No `is_razer`, no `has_razer`, no `Unit::Razer`, no
`RAZER_EU1_GUID` outside `src/models/razer_kiyo_pro.rs`.

- [ ] **The snapshots never moved**

```bash
git log --oneline --stat -- src/snapshots/
```

Expected: no commits from this work touch `src/snapshots/`.

- [ ] **A 0.2.0 profile still applies everything it used to**

With a Kiyo Pro attached and a profile written by 0.2.0 restored from backup:

```bash
cargo run -- apply
```

Expected: the applied list contains `hdr` and every standard control it
contained before the upgrade. This is ADR 0004's contract.
