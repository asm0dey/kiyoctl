# Match camera Models by vid:pid, not by extension-unit GUID

A Model is selected for an attached Camera by matching its USB vendor:product
id. The extension-unit GUID plays no part in selection; it only addresses which
Unit a Control lives on, via `Unit::Extension(guid)`.

This reverses how shipped kiyoctl behaves, so it is worth recording why.

## Context

Extension-unit GUIDs are not unique to a vendor. The Dell UltraSharp WB7022
(`413c:c015`) carries the *byte-identical* GUID to the Razer Kiyo Pro
(`1532:0e05`) — `23e49ed0-1178-4f31-ae52-d2fb8a8d3b48` — and accepts entirely
different payloads on it. kiyoctl through 0.2.0 matched on GUID alone, so it
identified a WB7022 as a Kiyo Pro and would write Razer payloads into Dell's
extension unit.

Conversely a single Logitech camera carries controls across four different
GUIDs at once, so a GUID is not even a one-per-Camera quantity.

## Consequences

Selection is exact, so there is no first-match-wins rule, no ordering
significance in the registry, and no "matches any camera with this GUID" case.
Two Models claiming the same vid:pid is a registry bug, caught by a self-check
test rather than by a precedence rule.

A rebadged or OEM camera that carries a known GUID but an unlisted vid:pid loses
its extension controls until its id is added. This is the intended failure: the
Dell case demonstrates that the same GUID and the same bytes mean different
things on different hardware, so declining to act is correct and guessing is
not.

A Control whose GUID is absent from the attached Camera resolves to no unit id
and is reported unavailable through the same path as a UVC control the camera
does not implement.
