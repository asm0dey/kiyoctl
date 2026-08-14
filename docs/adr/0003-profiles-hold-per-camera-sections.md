# Profiles hold per-camera sections for Model control values

A Profile keeps Standard control values in one flat map that applies to any
Camera, and Model control values in a section keyed by vid:pid. `apply` writes
the flat map plus the section matching the attached Camera. `capture` moves a
Profile's foreign Model values into their own section rather than dropping them,
and warns when the Camera it is capturing from is not the one the Profile was
last captured from.

## Context

Opaque controls are the only values in a Profile that cannot be recovered from
hardware — every Standard control can be re-read off the Camera, and an Opaque
one cannot be read off anything. A flat, single-camera Profile therefore loses
data permanently the first time it is captured on a different Camera: the old
Camera's HDR and field-of-view settings are overwritten by a capture that had no
way to read them.

Shipped kiyoctl makes this worse silently. `capture` skips Opaque controls but
unconditionally overwrites the Profile's recorded Camera identity, producing a
file that claims to be from one Camera while carrying another's Opaque values,
with no message to the user.

## Migration

Existing Profiles carry Model control values — `hdr`, `fov` — in the flat map,
because sections did not exist when they were written. Applying the flat map as
Standard-controls-only would silently stop applying them for every existing
user, so migration is not optional and is the load-bearing part of this ADR.

On load, a value found in the flat map moves into the section for the Camera the
Profile records **only when that Camera's own Model declares it**. Old Profiles
record the Camera in `device`, read through a serde alias on `camera`. The pass
is deterministic and invisible; the file is rewritten on next save.

Everything else stays flat, where it goes on applying to any Camera whose Model
declares it. Two cases reach that rule, and one governs both: a value that cannot
be read back from hardware is never worth relocating on a guess, because a
mis-homed one is unrecoverable and silent.

- **No recorded Camera.** Possible, since `Profile::default()` leaves it unset
  and the set-and-save path never fills it. There is nothing to key a section by.
- **A Camera that does not have the control.** This is the chimera file from the
  Context above, and it is the common case rather than a curiosity: 0.2.0's
  `cmd_set` overwrote the recorded identity on every Camera it touched, so a
  Kiyo Pro owner who ran one `set` against a second webcam has a Profile naming
  that webcam while carrying the Razer's flat `hdr`. Homing `hdr` to the webcam
  would stop it ever applying to the Kiyo Pro again — a downgrade for the exact
  population this ADR exists to protect. A Profile recording a Camera with no
  Model at all, or an identity that does not parse as `vvvv:pppp`, is the same
  case: nothing moves.

Which is to say: migration only ever moves a value it can prove belongs where it
is going. A Model control that stayed flat is not a failure — `apply` merges the
flat map with the attached Camera's section, so it is still written.

## Consequences

A Profile becomes "my look", portable across every Camera the user owns, with
each Camera's vendor settings remembered separately. The login agent applies the
right vendor settings on reconnect without being told which Camera arrived.

Existing Profiles keep behaving exactly as they did before the upgrade. A
Profile written by this version and read by 0.2.0 loses its Model values
silently, because serde ignores unknown fields; this is a downgrade path, not an
upgrade one.

This is the one place in the device-SPI work where structure was added for a
case that has not yet occurred. It is justified by the data being unrecoverable
rather than merely inconvenient to rebuild.
