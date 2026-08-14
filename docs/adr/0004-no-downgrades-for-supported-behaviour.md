# No downgrades for supported behaviour

An upgrade must not take anything away from a user of a Camera kiyoctl claims to
support. Where a change would remove or weaken behaviour those users depend on,
the change carries a migration instead — silently, without asking them to
re-do work.

This governed three decisions in the device-SPI design and is recorded because
the reasoning is invisible in the resulting code.

## What it required

Existing Profiles keep applying their Model control values, via the load-time
migration in ADR 0003. Without it every existing user's HDR and field-of-view
settings would have stopped applying, and they would have found out weeks later.

`controls` keeps printing every Model's controls in full. An earlier
decision to shrink it to a one-line index would have been strictly worse for
every user who exists today, in exchange for a scaling problem that starts at
four registry entries.

## Where the edge is

The constraint covers behaviour kiyoctl claims. It does not extend to behaviour
that was never a supported claim and was wrong.

The Dell UltraSharp WB7022 is the case that defines this. Matching by
extension-unit GUID (ADR 0002) made kiyoctl identify a WB7022 as a Kiyo Pro and
offer `hdr`, `fov` and the rest — controls that wrote Razer payloads into Dell's
unit and did not do what their labels said. Matching by vid:pid removes them.
That is not a downgrade under this ADR: the WB7022 was never a supported Camera,
it was an accidental match on a colliding GUID, and preserving the accident
would mean preserving the wrong bytes.

Dell's real payloads are published and porting them was considered and declined.
Supporting that Camera was never a goal, and the Device SPI exists precisely so
that someone who owns one can add it.
