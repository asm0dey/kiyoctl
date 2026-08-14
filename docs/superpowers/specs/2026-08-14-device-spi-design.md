# Device SPI

**Date:** 2026-08-14
**Status:** approved, not yet implemented
**Supersedes:** the first draft of this file, committed in `957c1a0`

Decisions recorded separately: [ADR 0001](../../adr/0001-relicense-to-gpl-3.md)
(relicense), [ADR 0002](../../adr/0002-match-models-by-usb-id.md) (matching),
[ADR 0003](../../adr/0003-profiles-hold-per-camera-sections.md) (profile
sections), [ADR 0004](../../adr/0004-no-downgrades-for-supported-behaviour.md)
(no downgrades). Vocabulary: [CONTEXT.md](../../../CONTEXT.md).

## Problem

Every vendor-specific fact in kiyoctl is spelled `Razer`. The extension-unit
GUID sits in the transport layer (`src/device.rs`), the write-only payloads sit
in `controls.rs` behind `Kind::Razer`, and thirty-nine call sites across
`main.rs` and `tui.rs` branch on `is_razer()` or `has_razer_unit()`.

Make the vendor-specific part a data structure that lives in one file, so that
somebody who owns a camera kiyoctl doesn't support — or an agent working on
their behalf — can add it by writing that file and one registry line.

Adding a camera is not the hard part of adding a camera. Obtaining the GUID,
selectors and payloads is, and nothing in this design produces that knowledge.
What it can do is make the observable half discoverable (`probe`), tell the user
their camera has something behind it (unrecognised-unit reporting), and make the
writing-it-down half mechanical.

### What the second camera actually looks like

Four vendors are already decoded in
[cameractrls](https://github.com/soyersoyer/cameractrls). Their shapes, not
guesses, are what this SPI is designed against:

| Vendor | GUID | Read-back | Selectors | Payloads |
| --- | --- | --- | --- | --- |
| Razer Kiyo Pro | `d09ee423…3b48` | write-only | one shared | 8-byte |
| Dell UltraSharp WB7022 | `d09ee423…3b48` — identical to Razer | write-only | one shared | 8-byte |
| Logitech | four GUIDs, simultaneously | readable | per-control | ints, menus |
| AnkerWork C310 | `a29e7641…003b` | readable | per-control | variable-length |

Three consequences the first draft got wrong: a GUID does not identify a vendor,
a camera is not limited to one extension unit, and an extension unit is not
synonymous with write-only.

## Non-goals

- **Porting Logitech, Dell or AnkerWork.** The SPI ships shapes, not vendors.
  Those four are the design's test corpus, checked on paper. Nobody's currently
  working behaviour depends on them, so adding them is a contributor's job.
- **Runtime-loadable device descriptions.** In-tree Rust entries are
  compile-time checked and cost no parser, no schema, no format stability
  contract.
- **A `trait Device`.** There is one implementation today; every hook would be a
  guess about the second implementer's needs.
- **`kiyoctl raw`** — arbitrary payload writes. An extension unit can expose
  firmware and calibration selectors; blind writes can brick hardware rather
  than merely wedge it. Nobody has asked for it.
- **Renaming the crate or binary.** The name stays `kiyoctl`.

## The SPI

A camera implementation is a const, not a trait.

```rust
// src/models/mod.rs

pub struct Model {
    /// Shown in the TUI header, in `list`, and as a `controls` heading.
    pub name: &'static str,
    /// The vid:pid pairs this Model covers. Required, non-empty. This is the
    /// only thing matching looks at — see ADR 0002.
    pub usb_ids: &'static [(u16, u16)],
    /// Controls this camera adds on top of the standard UVC catalogue.
    pub controls: &'static [Control],
    /// Unit, selector and payload that tell the camera to keep its extension
    /// unit state across a power cycle.
    pub persist: Option<(Unit, u8, &'static [u8])>,
}

pub static MODELS: &[Model] = &[razer_kiyo_pro::MODEL];
```

Adding a camera: copy `src/models/razer_kiyo_pro.rs`, change the ids, GUIDs and
payloads, add one line to `MODELS`.

There is no `Override`, no `hide`, no `range`, no `guid` field. Overrides were
cut as speculative — no camera is known to hide a control or misreport a range,
and the argument that killed `trait Device` kills a struct whose fields are
guesses about the next implementer just as well. When such a camera turns up,
`Override` is a small addition to a struct that is already data.

### A GUID addresses a unit; it does not select a Model

The first draft used the GUID for both jobs. Splitting them is what makes
Logitech representable and what fixes the Dell bug.

```rust
pub enum Unit {
    Camera,
    Processing,
    Extension(&'static [u8; 16]),
}

Control { name: "hdr",      unit: Unit::Extension(&RAZER_EU1), .. }
Control { name: "led_mode", unit: Unit::Extension(&LOGITECH_PERIPHERAL), .. }
Control { name: "brio_fov", unit: Unit::Extension(&LOGITECH_BRIO), .. }
```

A Model may name as many extension units as it likes. `Cam::unit_id` looks the
GUID up among the units actually found on the attached camera and returns `None`
when it is absent, which routes into the existing "camera does not implement
this control" path in `controls::read` and `Cam::info`. A Logitech without the
motor unit simply shows no pan/tilt controls, with no new code.

### Matching: vid:pid, exactly

A Model matches a Camera when the Camera's vid:pid appears in its `usb_ids`.
Nothing else is consulted.

The Dell UltraSharp WB7022 (`413c:c015`) carries the byte-identical GUID to the
Kiyo Pro (`1532:0e05`) and takes entirely different payloads on it. GUID
matching is therefore not merely insufficient, it is wrong, and it is wrong in
shipped 0.2.0 today.

Exact matching removes every ordering question the first draft had to answer:
no first-match-wins, no "empty means any camera with the GUID", no `guid: None`
case, no registry entry that could match everything. Two Models claiming the
same vid:pid is a bug caught by a test, not resolved by precedence.

The cost is that a rebadged camera carrying a known GUID under an unlisted pid
loses its extension controls until its id is added. That is the intended
failure. Only one Razer id exists (`1532:0e05`), so no Kiyo Pro owner is
affected.

### `Kind::Razer` becomes `Kind::Opaque`

```rust
pub struct OpaqueOpt {
    pub name: &'static str,
    pub payload: &'static [u8],
    pub pre: Option<&'static [u8]>,
}

pub enum Kind {
    Int { signed: bool },
    Bool,
    Menu(&'static [(&'static str, i64)]),
    Opaque(&'static [OpaqueOpt]),
}
```

`Opaque` means write-only named byte payloads with no read-back. It stops being
a synonym for "lives on the extension unit" — two facts that coincide only on
the Kiyo Pro and Dell.

A vendor whose extension unit answers `GET_CUR` — Logitech, AnkerWork — declares
`Kind::Int`, `Bool` or `Menu` on `Unit::Extension` and the existing read path
works unmodified. That separation is most of what makes this an SPI rather than
a rename.

`payload` and `pre` widen from `[u8; 8]` to `&'static [u8]` because AnkerWork
uses variable-length buffers. The Kiyo Pro entries change punctuation only. No
other `Kind` variant is added; AnkerWork's fire-and-forget button actions have
no clean representation and no user.

## Changes to existing code

### `src/device.rs` becomes `src/usb.rs`

It is the USB/UVC transport layer: `Cam`, `scan`, `fingerprint`, the control
requests. Leaving it as `device` would put `usb::Cam` next to `models::Model`
under a name that means neither. Roughly ten `use` sites.

### Vendor-neutral descriptor parsing

`parse_units` stops looking for one GUID and returns every extension unit it
finds, as `Vec<([u8; 16], u8)>` of GUID and unit id. `RAZER_EU1_GUID` moves out
of the transport layer into `src/models/razer_kiyo_pro.rs`.

`Cam` replaces `razer_id: Option<u8>` with `model: Option<&'static Model>` and
`extension_units: Vec<([u8; 16], u8)>`.

`Found` replaces `has_razer: bool` with `model: Option<&'static str>` and a
count of extension units no Model claims.

### The control list stops being one global static

| Name | Contents | Used by |
| --- | --- | --- |
| `controls::STANDARD` | the shared UVC catalogue, unchanged | the two below |
| `cam.controls()` | `STANDARD` plus the attached Model's | TUI, `show`, `capture`, `apply` |
| `controls::every()` | `STANDARD` plus every registered Model's | `controls`, unknown-control errors |

`controls::find` splits the same way: `cam.find(name)` for camera-bound
operations, `controls::find_any(name)` for name validation with no camera open.
`Profile::set` uses `find_any`.

List building is a pure function over `Option<&'static Model>`, not a method
needing a USB handle, which is what makes it testable without hardware:

```rust
pub fn effective_controls(model: Option<&'static Model>) -> Vec<&'static Control>;
```

Controls stay `&'static Control` throughout, so `tui::Row` and `profile::apply`'s
plan vector keep their current lifetimes.

### Splitting the `is_razer()` call sites

Each existing site meant one of two things:

- **"cannot be read back"** — `controls::read` bailing out, `profile::capture`
  skipping, the TUI painting the row magenta and showing the profile's
  remembered value. These become `is_opaque()`.
- **"only exists on this camera"** — `apply`'s availability check, `main.rs`'s
  per-camera help section. These become "the attached Model declares it."

### The persist hook, deduplicated

Today it is written three times — `profile.rs:222`, `main.rs:346`,
`tui.rs:632` — each hardcoding selector `0x01` and `RAZER_SAVE` rather than
reading them from the Model.

It becomes one `Cam::persist()`, called once per operation that wrote at least
one extension-unit control, issuing the attached Model's `persist` triple if it
declares one. A TUI edit counts as one operation, so its current per-edit
behaviour is unchanged: a TUI click is a CLI invocation.

## Profiles

Per ADR 0003. A Profile keeps Standard control values in one flat map that
applies to any Camera, and Model control values in a section keyed by vid:pid.

```json
{
  "camera":     "1532:0e05",
  "name":       "Razer Kiyo Pro",
  "controls":   { "brightness": 128, "white_balance_auto": "on" },
  "per_camera": { "1532:0e05": { "hdr": "on", "fov": "wide" } }
}
```

`Profile.device` becomes `Profile.camera`, with `#[serde(alias = "device")]`.

**Migration is not optional and is the load-bearing part of this work.**
Existing profiles carry `hdr` and `fov` in the flat map. On load, a Model
control found there moves into the section for the camera the profile records.
A profile with no recorded camera keeps its Model values flat and keeps applying
them. Without this pass every existing user's HDR silently stops applying.

`apply` writes the flat map plus the section matching the attached camera, and
notes when the profile was captured from a different one:

```
this profile was captured from a Razer Kiyo Pro (1532:0e05);
applying to Logitech BRIO (046d:085e) — 4 controls skipped
```

`capture` moves a profile's foreign Model values into their own section rather
than overwriting the profile's identity and stranding them. That closes a live
bug: today `capture` skips opaque controls but unconditionally rewrites
`profile.device` and `profile.name` (`src/profile.rs:150`), producing a file
claiming one camera while carrying another's values.

`apply` also gains the distinction the first draft identified: a name in
`controls::every()` but not in `cam.controls()` is "not available on this
camera"; only a name in neither is "unknown control".

## User-visible behaviour

### `kiyoctl probe`

New, read-only. For each extension unit on the selected camera, walks selectors
`0x01..0xff`, issues `GET_INFO` and `GET_CUR`, and prints what answered — the
GUID, the vid:pid, and per responding selector its capability bits, data length,
and current bytes if readable, labelling those that refuse `GET_CUR` as
write-only. Plain fenced text, ready to paste into an issue.

No `--rust` codegen. A skeleton file with `// TODO` where the payloads go is an
invitation to an agent to invent them, which is the exact failure the
contributor documentation exists to prevent.

### Unrecognised extension units

`list` marks them:

```
Logitech BRIO   046d:085e  bus 0 addr 4  [unrecognised extension unit]
```

`show` gives the actionable detail at the bottom:

```
This camera has 1 extension unit kiyoctl does not recognise:
  212de5ff-3080-2c4e-82d9-f587d00540bd  on 046d:085e

Vendor-specific controls may exist behind it.
Adding support is a small file — see docs/adding-a-camera.md
```

The TUI mentions them in the help overlay only, keeping the browse view about
the camera in front of you and leaving the browse-view snapshots untouched.

### Text that changes

| Command | Now | After |
| --- | --- | --- |
| `list` | `[Razer extension unit]` | `[Razer Kiyo Pro]` |
| `controls` | `-- Razer Kiyo Pro only, and write-only --` | `-- Razer Kiyo Pro --`, one group per Model |
| `show` | `Note: HDR and other Razer settings…` | same, Model name substituted |

`controls` keeps printing every Model's controls in full. Shrinking it to a
one-line index was considered and rejected under ADR 0004: with one Model it is
strictly worse for every user who exists today.

The TUI's `(magenta = write-only, remembered by kiyoctl)` note names no vendor.
It is re-keyed from "camera has a Razer unit" to "any opaque row is present" and
its rendered text is unchanged.

## The contributor path

`docs/adding-a-camera.md`, plus a heavily commented `src/models/_template.rs`.

The doc opens in the imperative, because the failure mode is specific and every
registry test passes on invented bytes:

> Do not guess payload bytes. If you do not have them from a USB capture, a
> vendor tool trace, or an existing open-source implementation, stop and open an
> issue with your `kiyoctl probe` output instead.

Every Model file carries a provenance comment at the top naming where its bytes
came from — an upstream project, a capture, or "dumped from my own hardware."
That is what makes a PR reviewable for a maintainer who does not own the camera,
and what keeps ADR 0001's licence position auditable.

A pull request adding a Model needs `probe` output and a named source.

`README.md` gains a section on adding a camera and its Razer-specific prose is
adjusted to describe the registry. The licence changes to GPL-3.0-or-later per
ADR 0001; `Cargo.toml`, `LICENSE` and the README header all move together.

## Testing

Hardware-free, in `src/models/mod.rs` and `src/profile.rs`.

1. **Registry self-check.** Every Model declares a non-empty `usb_ids`; no
   vid:pid is claimed by two Models; no Model repeats a control name within its
   own list; no Model control shadows a `STANDARD` name.
2. **No functional change.** `effective_controls(Some(&RAZER_KIYO_PRO))` equals
   today's `CONTROLS` contents in today's order. The regression guard for the
   whole refactor.
3. **Matching.** A known vid:pid selects its Model; an unknown one yields
   `None`; a camera carrying a known GUID under an unlisted pid yields `None`.
4. **Unit addressing.** A control whose `Extension` GUID is absent from the
   camera's unit list reports unavailable rather than writing to unit zero.
5. **Profile migration.** An old-format profile with `hdr` in the flat map and
   `device: "1532:0e05"` migrates into `per_camera`; one with no recorded camera
   keeps its values flat; the resulting apply plan is identical in both cases to
   what 0.2.0 would have produced.
6. **Cross-camera capture.** Capturing into a profile that names a different
   camera re-homes the foreign Model values into their own section rather than
   dropping them, and warns.

The TUI's `ui()` fixture changes `has_razer: true` to a Model field. The eight
insta snapshots must not move. A moved snapshot means a real behaviour change
and stops the work for review rather than being accepted.

`cargo build` and `cargo test` must pass with no new warnings.

## Risks

**A large mechanical diff across `main.rs` and `tui.rs`.** Mitigated by test 2,
which pins the effective control list, and by the snapshot tests, which pin the
TUI rendering.

**Profile migration is the one place a bug is silent.** A user whose HDR quietly
stops applying will not notice for weeks and will not connect it to an upgrade.
Test 5 is the guard, and it compares against 0.2.0's plan rather than against
the new code's own idea of correct.

**`Kind::Opaque` may still be too narrow.** AnkerWork's button actions already
do not fit. The second implementer extends `Kind`, which is a contained change
now that everything else is data.

**Dell UltraSharp WB7022 owners lose controls they appear to have today.**
Deliberate, per ADR 0004: those controls write Razer payloads into Dell's unit
and do not do what their labels say. Dell was never a supported camera.
