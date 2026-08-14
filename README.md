# kiyoctl

Command-line control of USB webcam settings on macOS, with saved profiles that
are reapplied at login and whenever the camera is reconnected.

Works with any UVC camera for standard controls. On the **Razer Kiyo Pro** it
additionally exposes HDR, HDR mode, field of view, and autofocus responsiveness
through Razer's vendor extension unit.

## How it works

UVC controls are class-specific requests on the USB default control pipe, so
kiyoctl never claims an interface and never conflicts with the system camera
driver or with apps using the camera. No sudo, no kernel extension.

```
kiyoctl ──libusb──► control endpoint ──► ┌─ camera terminal   (exposure, focus, zoom)
                                        ├─ processing unit   (brightness, white balance)
                                        └─ Razer ext. unit   (HDR, field of view)
```

## Interactive UI

Run `kiyoctl` with no arguments (or `kiyoctl tui`):

```
┌ kiyoctl ──────────────────────────────────────────────────────────────────┐
│Razer Kiyo Pro  1532:0e05   profile: default  * unsaved                   │
└──────────────────────────────────────────────────────────────────────────┘
┌ controls (magenta = write-only, remembered by kiyoctl) ───────────────────┐
│ > brightness             129  ████████████████················  0..255   │
│   white_balance_auto     off  off on                                     │
│   white_balance         4430  ██████████████··················  2000..75 │
│   exposure_time         1331  █████████████████████···········  (locked) │
│   hdr                     on  off on                                     │
│   hdr_mode                 ?  dark bright                                │
└──────────────────────────────────────────────────────────────────────────┘
```

| Key | Action |
| --- | --- |
| `↑ ↓` / `j k` | move between controls |
| `← →` / `h l` | step a number, cycle a choice |
| `Shift + ← →` | step by ten |
| `Enter` | type an exact number (or cycle a choice) |
| `d` | set to the camera's default |
| `p` | switch profile |
| `s` | save everything to the profile |
| `a` | apply the saved profile |
| `R` | restore all camera defaults |
| `r` | re-read from the camera |
| `?` | help · `q` quit |

### Mouse

| Action | Effect |
| --- | --- |
| click a row | select it |
| click a bar | jump straight to that value (the ends give the exact min and max) |
| drag along a bar | sweep the value, staying on the control you started on |
| click an option name | select that option |
| scroll | move the selection |
| click the profile name | open the profile picker |

Values snap to the control's resolution, so dragging across `white_balance`
yields multiples of ten rather than anything the camera would refuse.

Because mouse reporting is on while the UI runs, hold `Shift` if you want to
select text with the terminal's own selection instead.

### Profiles

Click the underlined profile name in the header (or press `p`) to switch
profiles or make a new one. Choosing an existing profile loads it **and applies
it to the camera**; `+ new profile…` asks for a name and saves the camera's
current settings under it, carrying over the write-only values that only kiyoctl
knows about. Names are restricted to letters, digits, `-` and `_`.

The profile you pick here becomes the one everything else uses — later `kiyoctl`
commands and the login agent — until you pick another. Nothing to re-apply
elsewhere.

Changes reach the camera the moment you make them, so you can watch the effect
in any app showing the picture. Nothing is written to the profile until `s` —
the header shows `* unsaved` while there are pending changes.

A control whose prerequisite is not met is dimmed and marked `(locked)`; the
status line says what to change to unlock it.

## Usage

```sh
kiyoctl list                    # attached cameras
kiyoctl show                    # every control your camera supports, with values
kiyoctl controls                # what each control means and accepts

kiyoctl get brightness
kiyoctl set brightness 140
kiyoctl set hdr on
kiyoctl set hdr=on fov=wide brightness=140    # several at once
```

Every `set` is remembered in the profile automatically. Use `--no-remember` to
change the camera without recording it.

### Profiles

```sh
kiyoctl save                    # snapshot the camera's current settings
kiyoctl apply                   # write the profile back to the camera
kiyoctl profiles                # list what is stored, marking the one in use
kiyoctl use streaming           # make that one the profile everything uses
kiyoctl reset                   # restore the camera's own defaults
```

Profiles live in `~/.config/kiyoctl/profiles/<name>.json` and are plain JSON you
can edit.

### Which profile a command uses

Commands act on the profile **in use**, which is whichever you last chose — in
the UI, or with `kiyoctl use`. `kiyoctl use` also puts it on the camera straight
away, exactly as choosing it in the UI does. The choice is remembered in
`~/.config/kiyoctl/active`, so it survives reboots and applies to the login agent
too.

`--profile <name>` overrides that for one command without changing what is in
use:

```sh
kiyoctl --profile streaming apply   # this once
kiyoctl use streaming               # from now on
```

### Persistence across reboots

```sh
kiyoctl install                 # login agent that keeps the profile applied
kiyoctl uninstall
```

`install` copies the binary to `~/.local/bin/kiyoctl` and writes a launchd agent
at `~/Library/LaunchAgents/local.kiyoctl.plist`. The agent polls for the camera
(descriptors only — it does not open the device unless something changes) and
applies the profile when it appears. Activity is logged to
`~/.config/kiyoctl/kiyoctl.log`.

The agent applies whichever profile is in use, so switching profiles in the UI
changes what it restores — no reinstall needed. Give `install` an explicit
`--profile` to pin it to one profile instead, whatever you are working with
interactively:

```sh
kiyoctl install                        # follows the profile in use
kiyoctl --profile streaming install    # pinned to 'streaming'
```

`--interval` sets how often it checks.

### Controlling the agent

```sh
kiyoctl daemon status           # running? with which profile and interval?
kiyoctl daemon stop             # unload it until the next login
kiyoctl daemon start
kiyoctl daemon restart
kiyoctl daemon reload           # re-read the profile and apply it right now
```

`reload` sends the running agent a `SIGHUP`, so an edit to
`~/.config/kiyoctl/profiles/<name>.json` takes effect immediately instead of
waiting for the camera to be replugged. It reaches the camera within a second
however long `--interval` is.

Switching profiles needs none of this: the agent notices within one interval and
adopts the new one — silently, because whoever switched has already applied it.
The `--device` and `--interval` settings do come from the plist, so changing
those means `kiyoctl install` again; `restart` alone will not pick them up.

`kiyoctl status` is a short form of `kiyoctl daemon status`, and `kiyoctl daemon`
with no subcommand runs the watch loop in the foreground, which is what the
launchd agent itself does.

## Two things worth knowing about the Kiyo Pro

**The Razer controls are write-only.** The camera accepts HDR / FoV / AF
commands but will not report their current state — there is no way to ask it
what HDR is set to. So `kiyoctl show` displays `?` for them, and `kiyoctl save`
cannot capture them. Instead, kiyoctl records the value whenever *you* set it, so
after `kiyoctl set hdr on` the profile knows about it from then on.

**HDR is also stored in the camera itself.** After changing a Razer setting,
kiyoctl sends the vendor "save" command, which writes it to the camera's own
non-volatile memory. Those settings often survive a power cycle without any
software running. The standard controls (brightness, white balance, exposure)
do reset, which is what the login agent is really for.

## Control dependencies

Some controls are only writable when another is set a particular way — the
camera rejects them otherwise. kiyoctl orders `apply` so the gating controls land
first, and skips a dependent control with an explanation rather than reporting a
spurious failure:

| Control | Requires |
| --- | --- |
| `white_balance` | `white_balance_auto=off` |
| `focus` | `focus_auto=off` |
| `exposure_time` | `auto_exposure=manual` or `shutter_priority` |

## If the camera stops responding

The Kiyo Pro's firmware can wedge: it stays enumerated on the USB bus but stops
answering control requests, and `kiyoctl` reports

> the camera is attached but is not answering USB control requests — unplug it
> and plug it back in, then try again

Unplugging and replugging is the only reliable fix. A software `libusb` reset
does not clear it. This is a known fault in the device rather than in kiyoctl —
see [kiyo-xhci-fix](https://github.com/jphein/kiyo-xhci-fix), which exists to
work around the same bug on Linux.

## Building and testing

```sh
cargo build --release
cargo test                  # unit and snapshot tests, no hardware needed
cargo test -- --ignored     # additionally renders against an attached camera
```

Needs Rust 1.85+ (2024 edition). libusb is built from source by `libusb1-sys`,
so there is no runtime dependency on Homebrew.

The UI is split so that [`Ui`](src/tui.rs) holds everything drawn on screen and
knows nothing about USB, while `App` owns the camera and performs writes. Key
handling returns an `Action` rather than touching hardware directly, so both the
layout and the interaction logic are tested without a webcam —
[insta](https://insta.rs) snapshots cover the rendering at two terminal sizes.

## Multiple cameras

`--device` accepts a `vvvv:pppp` USB id or any part of the product name:

```sh
kiyoctl --device 1532:0e05 show
kiyoctl --device kiyo set hdr on
```

## Credit

The Razer extension-unit protocol — the GUID, selectors, and payloads — was
worked out by the [kiyoproctrls](https://github.com/soyersoyer/kiyoproctrls) and
[cameractrls](https://github.com/soyersoyer/cameractrls) projects.
