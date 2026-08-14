# kiyoctl

Reads and writes UVC webcam settings over the USB control pipe, remembers them
in a profile, and reapplies them at login and on reconnect. Named for the Razer
Kiyo Pro, whose vendor extension unit it decodes, but it drives standard UVC
controls on any webcam and accepts contributed support for others.

## Language

### Hardware

**Camera**:
A physical webcam attached to the USB bus, identified by a vid:pid pair and a
bus address, usually carrying a product string.
_Avoid_: device, webcam, cam (as a domain term; `Cam` remains the type name for
an open camera's handle)

**Model**:
The static, vendor-specific knowledge about one kind of camera: which vid:pid
pairs it covers, the controls it adds, and what payloads they take. Selected for
an attached Camera by vid:pid alone — never by GUID, see ADR 0002.
_Avoid_: Device, DeviceSpec, driver, profile

**Camera Selector**:
User-supplied text that picks one attached Camera when several are present —
either `vvvv:pppp` or a case-insensitive substring of the product string. What
`--device` takes.
_Avoid_: device, device id, device string

**Unit**:
A logical block inside a Camera that owns a set of Controls: the camera terminal
(optics), the processing unit (image pipeline), or an Extension Unit.

**Extension Unit**:
A vendor-defined Unit, identified by a 16-byte GUID in the VideoControl
descriptors. One Camera may carry several; one GUID may appear on Cameras from
different vendors meaning entirely different things.
_Avoid_: Razer unit, EU1, vendor unit

**Unrecognised extension unit**:
An Extension Unit present on an attached Camera that no Model claims. Reported
to the user with its GUID, because it is the signal that contributed support is
possible.

**Probe**:
Reading an attached Camera's Extension Units and walking their selectors to
report which exist, which answer, and what they currently hold — without knowing
what any of it means. The evidence a contribution is built from.

### Controls

Two independent axes: where a Control comes from, and whether it can be read
back.

**Control**:
One named setting kiyoctl can address — a Unit, a selector byte, a data length,
and a value domain.

**Standard control**:
A Control from the shared UVC catalogue. Present on any Camera that implements
it, independent of any Model.

**Model control**:
A Control declared by a Model. Only meaningful on Cameras that Model matches.
_Avoid_: vendor control, extension control (a Model control usually lives on an
Extension Unit, but that is a separate fact)

**Opaque control**:
A Control written as a fixed named byte payload that cannot be read back; the
Camera accepts the write but never reports the value. Its state is known only
from the Profile. Orthogonal to Standard/Model — a Model control may be
perfectly readable.
_Avoid_: Razer control, write-only control, blind control

### Persistence

**Profile**:
A named set of Control values saved as JSON under `~/.config/kiyoctl/profiles/`,
keyed by Control name. Holds Standard control values that apply to any Camera,
plus a section of Model control values per Camera it has been captured from.
_Avoid_: preset, config, settings file

**Active profile**:
The Profile used when none is named on the command line. Chosen with `use` or in
the TUI, recorded on disk so it outlives the process.

**Capture**:
Reading an attached Camera's current values into a Profile. Opaque controls
cannot be captured and keep whatever the Profile already held.

**Apply**:
Writing a Profile's values to a Camera — its Standard control values, plus the
section matching that Camera — ordered so gating Controls land first, skipping
Controls whose prerequisites are unmet or that the Camera does not have.

**Persist**:
Asking a Camera to keep its Extension Unit state across a power cycle. Issued
once per operation that wrote at least one Extension Unit Control; a TUI edit
counts as one operation.
_Avoid_: save (collides with saving a Profile to disk)
