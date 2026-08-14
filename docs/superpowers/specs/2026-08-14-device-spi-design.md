# Device SPI

**Date:** 2026-08-14
**Status:** approved, not yet implemented

## Problem

Every vendor-specific fact in kiyoctl is spelled `Razer`. The extension-unit
GUID sits in the transport layer (`src/device.rs`), the write-only payloads sit
in `controls.rs` behind `Kind::Razer`, and roughly forty call sites across
`main.rs` and `tui.rs` branch on `is_razer()` or `has_razer_unit()`. A second
camera vendor cannot be added without editing all of them.

Make the vendor-specific part a data structure that lives in one file, so
adding a camera means writing that file and adding one line to a registry.

No behaviour changes for the Razer Kiyo Pro. No new dependencies.

## Non-goals

- Runtime-loadable device descriptions (TOML/JSON in the config directory).
  Rejected: it buys a parser, a schema, validation, error reporting for bad
  files, and a stability contract on the format, in exchange for skipping a
  rebuild. In-tree Rust entries are compile-time checked and cost nothing.
- A `trait Device`. There is one implementation today; every hook would be a
  guess about what the second implementer needs.
- Renaming the crate or the binary.

## The SPI

A device implementation is a const, not a trait.

```rust
// src/devices/mod.rs

pub struct Device {
    /// Shown in the TUI header and in `list-controls` section headings.
    pub name: &'static str,
    /// Extension-unit GUID to look for in the VideoControl descriptors.
    /// None for a device that only carries overrides.
    pub guid: Option<[u8; 16]>,
    /// Restrict to these vid:pid pairs. Empty means any camera with the GUID.
    pub usb_ids: &'static [(u16, u16)],
    /// Controls living on this device's extension unit.
    pub controls: &'static [Control],
    /// Adjustments to the shared UVC controls.
    pub overrides: &'static [Override],
    /// Selector and payload that tell the camera to persist extension-unit
    /// state across a power cycle. Issued after any extension control is
    /// written.
    pub save: Option<(u8, &'static [u8])>,
}

pub struct Override {
    pub control: &'static str,
    /// The camera advertises the control but it does nothing: drop it.
    pub hide: bool,
    /// Trust this instead of the camera's GET_MIN/GET_MAX.
    pub range: Option<(i64, i64)>,
}

pub static DEVICES: &[Device] = &[razer_kiyo_pro::DEVICE];
```

Adding a camera: copy `src/devices/razer_kiyo_pro.rs`, change the GUID and the
payloads, add one line to `DEVICES`.

### Why `Override` has no `rename` field

Saved profiles are keyed by control name, so renaming a standard control would
silently break them. Hide-plus-supply-your-own expresses the same intent
explicitly: `Override { control: "zoom", hide: true, range: None }` alongside a
`Control { name: "digital_zoom", .. }` in the device's own list.

### Matching: first match wins

A device matches a camera when both hold:

- its `guid` is `None`, or that GUID appears among the camera's extension units;
- its `usb_ids` is empty, or it contains the camera's vid:pid.

The first matching entry in `DEVICES` attaches to the `Cam`; no others are
consulted and nothing is merged. A camera needing both an extension unit and a
UVC quirk puts both in one entry. This avoids ordering rules and avoids "which
of my two entries supplied this control" as a debugging question.

An entry with neither `guid` nor `usb_ids` would match every camera on the bus.
The registry self-check test rejects it.

A `guid: None` entry has no extension unit to put controls on, so it must leave
`controls` empty and carry only `overrides`. The self-check test enforces this
too.

### `Kind::Razer` becomes `Kind::Opaque`

`Kind::Opaque` means write-only, named byte payloads, no read-back. It stops
being a synonym for "lives on the extension unit" — two facts that coincide
only on the Kiyo Pro.

A vendor whose extension unit answers `GET_CUR` declares `Kind::Int`, `Bool` or
`Menu` on `Unit::Extension`, and the existing read path works unmodified. That
separation is most of what makes this an SPI rather than a rename.

## Changes to existing code

### Rename `src/device.rs` to `src/usb.rs`

It is the USB/UVC transport layer: `Cam`, `scan`, `fingerprint`, the control
requests. Leaving it as `device` would put `device::Cam` next to
`devices::Device` permanently. Roughly ten `use` sites.

### Vendor-neutral descriptor parsing

`parse_units` stops looking for one GUID and returns every extension unit it
finds, as `Vec<([u8; 16], u8)>` of GUID and unit ID. `RAZER_EU1_GUID` moves
from the transport layer into `src/devices/razer_kiyo_pro.rs`.

`Cam` replaces `razer_id: Option<u8>` with `device: Option<&'static Device>`
and `extension_id: Option<u8>`. `Unit::Razer` becomes `Unit::Extension`.

`Found` replaces `has_razer: bool` with `device: Option<&'static str>`.

### The control list stops being one global static

`controls::CONTROLS` splits three ways:

| Name | Contents | Used by |
| --- | --- | --- |
| `controls::STANDARD` | the shared UVC catalogue, unchanged | the two below |
| `cam.controls()` | `STANDARD` minus hidden, plus the device's own | TUI, `show`, `capture`, `apply` |
| `controls::every()` | `STANDARD` plus every registered device's | `list-controls`, the unknown-control error |

`controls::find` splits the same way: `cam.find(name)` for camera-bound
operations, `controls::find_any(name)` for name validation with no camera open.

List-building and range lookup are pure functions over `Option<&Device>`, not
methods needing a USB handle:

```rust
pub fn effective_controls(dev: Option<&'static Device>) -> Vec<&'static Control>;
pub fn range_for(dev: Option<&Device>, name: &str, from_camera: Option<(i64, i64)>)
    -> Option<(i64, i64)>;
```

`Cam` delegates to them. This is what makes them testable without hardware.

Controls stay `&'static Control` throughout, so `tui::Row` and
`profile::apply`'s plan vector keep their current lifetimes. Overrides are
looked up by name at read time rather than baked into cloned `Control` values.

### Splitting the `is_razer()` call sites

Each existing call site meant one of two things:

- "cannot be read back" — `controls::read` bailing out, `profile::capture`
  skipping, the TUI showing the profile's remembered value and painting the row
  magenta. These become `is_opaque()`.
- "only exists on this camera" — `apply`'s availability check, `main.rs`'s
  per-device help section. These become `unit == Unit::Extension` plus a device
  being attached.

The post-write persist hook becomes: any `Unit::Extension` control written
triggers the attached device's `save`, if it declares one. Today's unconditional
`RAZER_SAVE` write disappears.

### Profiles are untouched

Profile JSON is keyed by control name; neither `Unit` nor `Kind` is serialized.
Every existing profile keeps working byte for byte.

One improvement falls out. Applying a Kiyo Pro profile to a different camera
currently reports `hdr` as `unknown control`. It can now distinguish: a name in
`controls::every()` but not in `cam.controls()` is "not available on this
camera", and only a name in neither is "unknown control".

## User-visible text changes

Three strings hardcode the vendor and cannot survive a second device:

| Command | Now | After |
| --- | --- | --- |
| `list` | `[Razer extension unit]` | `[Razer Kiyo Pro]` |
| `list-controls` | `-- Razer Kiyo Pro only, and write-only --` | `-- Razer Kiyo Pro only --` |
| `show` | `Note: HDR and other Razer settings cannot be read back...` | same, with the device name substituted |

The TUI's `(magenta = write-only, remembered by kiyoctl)` note names no vendor.
It is re-keyed from "camera has a Razer unit" to "any opaque row is present",
and the rendered text is unchanged.

`README.md` gains a section on adding a device, and its existing Razer-specific
prose is adjusted to describe the registry.

## Testing

Four tests, all hardware-free, in `src/devices/mod.rs`.

1. **Registry self-check.** Every entry in `DEVICES` declares a `guid` or a
   non-empty `usb_ids`; a `guid: None` entry declares no `controls`; no device
   repeats a control name within its own list; a device control whose name
   shadows a `STANDARD` one must also hide it.
2. **No functional change.** `effective_controls(Some(&RAZER))` equals today's
   `CONTROLS` contents in today's order. This is the regression guard for the
   whole refactor.
3. **Overrides.** A `hide` override removes the control from
   `effective_controls`; a `range` override wins over the value passed as the
   camera's reported range, and `range_for` falls through to the camera's value
   when no override applies.
4. **Matching.** GUID-only, USB-ID-only, and both-must-hold entries each select
   the right device; a camera matching nothing yields `None`; when two entries
   could match, the earlier one in `DEVICES` wins.

The TUI's `ui()` fixture changes `has_razer: true` to a device-name field. The
eight insta snapshots must not move. A moved snapshot means a real behaviour
change and stops the work for review rather than being accepted.

`cargo build` and `cargo test` must pass with no new warnings.

## Risks

**A large mechanical diff across `main.rs` and `tui.rs`.** Mitigated by test 2,
which pins the effective control list, and by the snapshot tests, which pin the
TUI rendering.

**`Kind::Opaque` may still be too narrow for the second vendor.** A vendor
needing, say, a read-modify-write payload has no way to express it. That is
acceptable: the second implementer extends the `Kind` enum, which is a
contained change now that everything else is data.
