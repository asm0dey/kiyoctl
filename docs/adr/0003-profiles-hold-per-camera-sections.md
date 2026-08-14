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

On load, a Model control found in the flat map moves into the section for the
Camera the Profile records. Old Profiles record it in `device`, read through a
serde alias on `camera`. The pass is deterministic and invisible; the file is
rewritten on next save.

A Profile with no recorded Camera — possible, since `Profile::default()` leaves
it unset and the set-and-save path never fills it — keeps its Model values in
the flat map and keeps applying them to any Camera whose Model declares them.
Guessing which Camera they belonged to would be worse than leaving them put.

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
