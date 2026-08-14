# Relicense from MIT to GPL-3.0-or-later

kiyoctl is meant to accept camera Models contributed by other people and their
agents, and the only substantial published corpus of webcam extension-unit
protocols is [cameractrls](https://github.com/soyersoyer/cameractrls), which is
LGPL-3.0-or-later. Under MIT a contribution transcribed from that source would
not be license-compatible, so kiyoctl relicenses to GPL-3.0-or-later, which
accepts LGPL-3.0 material inbound.

## Considered Options

**LGPL-3.0-or-later** was chosen first and reversed. LGPL exists to let
proprietary programs link a copyleft library; kiyoctl is a statically-linked
binary with no library consumers, so LGPLv3 §4's obligation to ship object code
"in a form suitable for relinking" would be a real burden on every release in
exchange for a freedom nobody can exercise.

**MPL-2.0** would have protected the device Models specifically — file-level
copyleft, unencumbered binary — but it cannot take LGPL-3.0 code inbound, which
defeats the reason for relicensing at all.

**Staying MIT** would mean only accepting device data from permissive sources or
from contributors who dumped it from their own hardware.

## Consequences

Releases 0.1.1 and 0.2.0 shipped under MIT and remain MIT permanently; anyone
may fork from those tags and continue permissively. The relicense is prospective
only.

This is a one-way door. It is available now because the repository has a single
author, and closes as soon as outside contributions land under the new terms.

The Kiyo Pro payloads already in the tree came from
[kiyoproctrls](https://github.com/soyersoyer/kiyoproctrls), which is MIT, and
were never encumbered.
