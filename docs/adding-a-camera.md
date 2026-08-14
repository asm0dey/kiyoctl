# Adding a camera

kiyoctl drives standard UVC controls on any webcam. Vendor-specific controls —
HDR, field of view, LED behaviour, pan and tilt — live behind an *extension
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
