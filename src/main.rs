//! kiyoctl — read and write UVC webcam settings, remember them, and put them
//! back after a reboot or a replug.

mod controls;
mod device;
mod profile;
mod service;
mod tui;

use clap::{Parser, Subcommand};
use controls::CONTROLS;
use device::Cam;
use profile::Profile;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Parser)]
#[command(
    name = "kiyoctl",
    version,
    about = "Control UVC webcam settings (including Razer Kiyo Pro HDR) and restore them automatically"
)]
struct Cli {
    /// Which camera: "vvvv:pppp" or part of its product name
    #[arg(long, global = true)]
    device: Option<String>,

    /// Profile to read from or write to, just this once. Without it, the
    /// profile last chosen in the UI (or with `kiyoctl use`) is used.
    #[arg(long, short, global = true)]
    profile: Option<String>,

    /// Defaults to the interactive UI when omitted
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Adjust settings interactively (the default with no subcommand)
    Tui,
    /// List attached UVC cameras
    List,
    /// Describe every control kiyoctl understands, and its accepted values
    Controls,
    /// Show every control the camera supports, with its current value
    Show,
    /// Print one control's current value
    Get { control: String },
    /// Set one or more controls, e.g. `set hdr on` or `set hdr=on brightness=140`
    Set {
        #[arg(required = true)]
        assignments: Vec<String>,
        /// Apply to the camera without recording it in the profile
        #[arg(long)]
        no_remember: bool,
    },
    /// Snapshot the camera's current settings into the profile
    Save,
    /// Write the profile back to the camera
    Apply,
    /// List saved profiles
    Profiles,
    /// Restore the camera's own default values
    Reset,
    /// Control the background agent, or run its watch loop in the foreground
    Daemon {
        /// Seconds between checks
        #[arg(long, default_value = "2")]
        interval: u64,
        /// Defaults to running the watch loop when omitted
        #[command(subcommand)]
        action: Option<DaemonCmd>,
    },
    /// Make a profile the one later commands and the login agent use
    Use { name: String },
    /// Install a login agent that keeps the profile applied
    Install {
        #[arg(long, default_value = "2")]
        interval: u64,
    },
    /// Remove the login agent
    Uninstall,
    /// Report whether the login agent is running (same as `daemon status`)
    Status,
}

#[derive(Subcommand)]
enum DaemonCmd {
    /// Watch for the camera in the foreground (what the login agent runs)
    Run,
    /// Report whether the agent is running, and what it is watching
    Status,
    /// Start the agent
    Start,
    /// Stop the agent until the next login
    Stop,
    /// Stop and start the agent
    Restart,
    /// Make the running agent re-read the profile and apply it now
    Reload,
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(&cli) {
        eprintln!("kiyoctl: {e}");
        std::process::exit(1);
    }
}

fn run(cli: &Cli) -> Result<(), String> {
    let dev = cli.device.as_deref();
    // An explicit --profile applies to this command only; otherwise use the
    // profile that was last chosen and recorded.
    let pinned = cli.profile.as_deref();
    let in_use = pinned.map(str::to_string).unwrap_or_else(profile::active);
    let Some(cmd) = &cli.cmd else {
        return tui::run(Cam::open(dev)?, &in_use);
    };
    match cmd {
        Cmd::Tui => tui::run(Cam::open(dev)?, &in_use),
        Cmd::List => cmd_list(),
        Cmd::Controls => cmd_controls(),
        Cmd::Show => cmd_show(dev),
        Cmd::Get { control } => cmd_get(dev, control),
        Cmd::Set { assignments, no_remember } => cmd_set(dev, &in_use, assignments, *no_remember),
        Cmd::Save => cmd_save(dev, &in_use),
        Cmd::Apply => cmd_apply(dev, &in_use),
        Cmd::Profiles => cmd_profiles(),
        Cmd::Use { name } => cmd_use(dev, name),
        Cmd::Reset => cmd_reset(dev),
        Cmd::Daemon { interval, action } => match action {
            None | Some(DaemonCmd::Run) => cmd_daemon(dev, pinned, *interval),
            Some(DaemonCmd::Status) => cmd_daemon_status(),
            Some(DaemonCmd::Start) => {
                service::start()?;
                service::settle();
                println!("Login agent started.");
                cmd_daemon_status()
            }
            Some(DaemonCmd::Stop) => {
                service::stop()?;
                println!("Login agent stopped. It starts again at the next login — `kiyoctl uninstall` removes it.");
                Ok(())
            }
            Some(DaemonCmd::Restart) => {
                service::restart()?;
                service::settle();
                println!("Login agent restarted.");
                cmd_daemon_status()
            }
            Some(DaemonCmd::Reload) => {
                // The agent's profile comes from the plist or from the recorded
                // selection — never from this command's --profile.
                let running = service::agent().and_then(|a| a.profile).unwrap_or_else(profile::active);
                service::reload()?;
                println!("Asked the login agent to re-read profile '{running}' and apply it.");
                println!("Log: {}", profile::config_dir().join("kiyoctl.log").display());
                Ok(())
            }
        },
        Cmd::Install { interval } => cmd_install(dev, pinned, &in_use, *interval),
        Cmd::Uninstall => {
            service::uninstall()?;
            println!("Login agent removed.");
            Ok(())
        }
        Cmd::Status => cmd_daemon_status(),
    }
}

fn cmd_daemon_status() -> Result<(), String> {
    let (profile_name, device, interval) = match service::agent() {
        Some(agent) => {
            match agent.pid {
                Some(pid) => println!("Login agent: {} (pid {pid})", agent.state),
                None => println!("Login agent: loaded but not running ({})", agent.state),
            }
            (agent.profile, agent.device, agent.interval)
        }
        None if service::installed() => {
            println!("Login agent: stopped. Start it with `kiyoctl daemon start`.");
            (
                service::installed_flag("--profile"),
                service::installed_flag("--device"),
                service::installed_flag("--interval"),
            )
        }
        None => {
            println!("Login agent: not installed. Install it with `kiyoctl install`.");
            return Ok(());
        }
    };

    match &profile_name {
        Some(pinned) => println!("  profile:  {pinned} (pinned at install)"),
        None => println!("  profile:  {} (whichever profile is in use)", profile::active()),
    }
    if let Some(device) = &device {
        println!("  device:   {device}");
    }
    println!("  interval: {}s", interval.as_deref().unwrap_or("2"));
    println!("  plist:    {}", service::plist_path().display());
    println!("  log:      {}", profile::config_dir().join("kiyoctl.log").display());
    Ok(())
}

fn cmd_list() -> Result<(), String> {
    let found = device::scan().map_err(|e| e.to_string())?;
    if found.is_empty() {
        println!("No UVC cameras found.");
        return Ok(());
    }
    for f in &found {
        let extra = if f.has_razer { "  [Razer extension unit]" } else { "" };
        println!(
            "{}  {:04x}:{:04x}  bus {} addr {}{}",
            f.name, f.vid, f.pid, f.bus, f.address, extra
        );
    }
    Ok(())
}

fn cmd_controls() -> Result<(), String> {
    let width = CONTROLS.iter().map(|c| c.name.len()).max().unwrap_or(0);
    let mut last_razer = false;
    for (i, ctrl) in CONTROLS.iter().enumerate() {
        if ctrl.is_razer() && !last_razer {
            println!("\n  -- Razer Kiyo Pro only, and write-only --");
        }
        last_razer = ctrl.is_razer();
        let values = match ctrl.choices() {
            Some(c) => c.join(" | "),
            None => "a number (range depends on the camera)".to_string(),
        };
        println!("  {:<width$}  {}\n  {:<width$}  values: {values}", ctrl.name, ctrl.help, "");
        if i + 1 < CONTROLS.len() {
            println!();
        }
    }
    println!("\nNot every camera implements every control; `kiyoctl show` lists what yours does.");
    Ok(())
}

fn cmd_show(dev: Option<&str>) -> Result<(), String> {
    let cam = Cam::open(dev)?;
    println!("{} ({:04x}:{:04x})\n", cam.name, cam.vid, cam.pid);

    let mut rows: Vec<(String, String, String)> = Vec::new();
    for ctrl in CONTROLS {
        if ctrl.is_razer() {
            continue;
        }
        if let Some(r) = controls::read(&cam, ctrl)? {
            let mut detail = String::new();
            if let Some((lo, hi)) = r.range {
                detail.push_str(&format!("{lo}..{hi}"));
                if let Some(step) = r.step {
                    if step > 1 {
                        detail.push_str(&format!(" step {step}"));
                    }
                }
            } else if let Some(c) = ctrl.choices() {
                detail.push_str(&c.join("|"));
            }
            if let Some(d) = &r.default {
                if !detail.is_empty() {
                    detail.push_str(", ");
                }
                detail.push_str(&format!("default {d}"));
            }
            if !r.writable {
                detail.push_str(" (read-only)");
            }
            rows.push((ctrl.name.to_string(), r.value, detail));
        }
    }

    // An empty list from a silent camera is a fault, not a plain camera.
    if rows.is_empty() && !cam.responding {
        return Err(device::NOT_RESPONDING.into());
    }

    let width = rows.iter().map(|(n, _, _)| n.len()).max().unwrap_or(0);
    let vwidth = rows.iter().map(|(_, v, _)| v.len()).max().unwrap_or(0);
    for (name, value, detail) in &rows {
        println!("  {name:<width$}  {value:>vwidth$}   {detail}");
    }

    if cam.has_razer_unit() {
        println!("\nRazer extension unit (write-only — the camera does not report these back):");
        for ctrl in CONTROLS.iter().filter(|c| c.is_razer()) {
            let choices = ctrl.choices().unwrap_or_default().join("|");
            println!("  {:<width$}  {:>vwidth$}   {choices}", ctrl.name, "?");
        }
    }
    Ok(())
}

fn cmd_get(dev: Option<&str>, name: &str) -> Result<(), String> {
    let ctrl = controls::find(name).ok_or_else(|| unknown_control(name))?;
    if ctrl.is_razer() {
        return Err(format!(
            "{name} is write-only; the camera cannot report it. Its last value is in the profile — see `kiyoctl profiles`."
        ));
    }
    let cam = Cam::open(dev)?;
    match controls::read(&cam, ctrl)? {
        Some(r) => {
            println!("{}", r.value);
            Ok(())
        }
        None => Err(format!("{} does not support {name}", cam.name)),
    }
}

fn cmd_set(dev: Option<&str>, profile_name: &str, args: &[String], no_remember: bool) -> Result<(), String> {
    let pairs = parse_assignments(args)?;
    let cam = Cam::open(dev)?;
    let mut prof = if no_remember { Profile::default() } else { Profile::load_or_default(profile_name)? };

    let mut touched_razer = false;
    for (name, value) in &pairs {
        let ctrl = controls::find(name).ok_or_else(|| unknown_control(name))?;
        if ctrl.is_razer() && !cam.has_razer_unit() {
            return Err(format!("{} has no Razer extension unit, so {name} is unavailable", cam.name));
        }
        controls::write(&cam, ctrl, value).map_err(|e| cam.explain(e))?;
        touched_razer |= ctrl.is_razer();
        println!("{name} = {value}");
        if !no_remember {
            prof.set(name, value);
        }
    }

    // Ask the camera to keep extension-unit settings in its own storage.
    if touched_razer {
        let _ = cam.set(device::Unit::Razer, 0x01, &controls::RAZER_SAVE);
    }

    if !no_remember {
        prof.device = Some(format!("{:04x}:{:04x}", cam.vid, cam.pid));
        prof.name = Some(cam.name.clone());
        prof.save(profile_name)?;
    }
    Ok(())
}

fn cmd_save(dev: Option<&str>, profile_name: &str) -> Result<(), String> {
    let cam = Cam::open(dev)?;
    // Start from what is already stored so write-only values are not lost.
    let mut prof = Profile::load_or_default(profile_name)?;
    let captured = profile::capture(&cam, &mut prof);
    let path = prof.save(profile_name)?;
    println!("Saved {} settings from {} to {}", captured.len(), cam.name, path.display());
    if cam.has_razer_unit() {
        let tracked: Vec<&str> = CONTROLS
            .iter()
            .filter(|c| c.is_razer() && prof.controls.contains_key(c.name))
            .map(|c| c.name)
            .collect();
        if tracked.is_empty() {
            println!(
                "Note: HDR and other Razer settings cannot be read back. Set them once with \
                 `kiyoctl set hdr on` and they will be remembered from then on."
            );
        } else {
            println!("Remembered Razer settings: {}", tracked.join(", "));
        }
    }
    Ok(())
}

fn cmd_apply(dev: Option<&str>, profile_name: &str) -> Result<(), String> {
    let cam = Cam::open(dev)?;
    let prof = Profile::load(profile_name)?;
    let report = profile::apply(&cam, &prof);
    for line in &report.applied {
        println!("{line}");
    }
    for (name, why) in &report.skipped {
        eprintln!("skipped {name}: {why}");
    }
    println!("\nApplied {} settings to {}.", report.applied.len(), cam.name);
    Ok(())
}

fn cmd_profiles() -> Result<(), String> {
    let names = Profile::list();
    if names.is_empty() {
        println!("No profiles yet. Create one with `kiyoctl save`.");
        return Ok(());
    }
    let active = profile::active();
    for name in names {
        let prof = Profile::load(&name)?;
        let device = prof.name.clone().unwrap_or_else(|| "unknown camera".into());
        let mark = if name == active { " * in use" } else { "" };
        println!("{name}  ({device}, {} settings){mark}", prof.controls.len());
        for (k, v) in &prof.controls {
            let rendered = match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            println!("    {k} = {rendered}");
        }
    }
    Ok(())
}

fn cmd_use(dev: Option<&str>, name: &str) -> Result<(), String> {
    let prof = Profile::load(name).map_err(|e| {
        let known = Profile::list();
        if known.is_empty() {
            e
        } else {
            format!("{e}\nSaved profiles: {}", known.join(", "))
        }
    })?;
    profile::set_active(name)?;
    println!("Profile '{name}' is now in use, for later commands and the login agent.");

    // Put it on the camera straight away, the way choosing one in the UI does.
    match Cam::open(dev) {
        Ok(cam) => {
            let report = profile::apply(&cam, &prof);
            for (control, why) in &report.skipped {
                eprintln!("skipped {control}: {why}");
            }
            println!("Applied {} settings to {}.", report.applied.len(), cam.name);
        }
        Err(e) => println!("Not applied now ({e}); it will be applied when the camera is next seen."),
    }
    Ok(())
}

fn cmd_reset(dev: Option<&str>) -> Result<(), String> {
    let cam = Cam::open(dev)?;
    let mut n = 0;
    for ctrl in CONTROLS {
        if ctrl.is_razer() {
            continue;
        }
        if let Ok(Some(r)) = controls::read(&cam, ctrl) {
            if r.writable {
                if let Some(def) = &r.default {
                    if controls::write(&cam, ctrl, def).is_ok() {
                        n += 1;
                    }
                }
            }
        }
    }
    println!("Restored {n} controls to the camera's defaults.");
    if cam.has_razer_unit() {
        println!("Razer settings (HDR, field of view) were left alone — set them explicitly if needed.");
    }
    Ok(())
}

/// Set by the SIGHUP handler; `kiyoctl daemon reload` is what sends it.
static RELOAD: AtomicBool = AtomicBool::new(false);

const SIGHUP: i32 = 1;

// Avoiding a libc dependency for a single call.
extern "C" {
    fn signal(sig: i32, handler: usize) -> usize;
}

extern "C" fn on_sighup(_sig: i32) {
    // Storing a flag is all that is safe to do from a signal handler.
    RELOAD.store(true, Ordering::SeqCst);
}

fn cmd_daemon(dev: Option<&str>, pinned: Option<&str>, interval: u64) -> Result<(), String> {
    let mut wanted = pinned.map(str::to_string).unwrap_or_else(profile::active);
    // Fail fast on a missing profile rather than looping uselessly.
    Profile::load(&wanted)?;
    // Without this, SIGHUP would simply kill the daemon.
    unsafe { signal(SIGHUP, on_sighup as extern "C" fn(i32) as usize) };
    log(&format!(
        "watching for camera every {interval}s, profile '{wanted}'{}",
        if pinned.is_some() { " (pinned)" } else { "" }
    ));

    // Poll a descriptor-only signature; only touch the camera when the set of
    // attached devices actually changes — or when asked to reload.
    let mut last = String::from("<start>");
    loop {
        let current = device::fingerprint();
        let reload = RELOAD.swap(false, Ordering::SeqCst);
        if reload {
            log("reload requested");
        }

        // Follow the profile chosen in the UI. Whoever chose it has already put
        // it on the camera, so adopt the name without writing anything now; it
        // takes effect at the next reload or the next time the camera appears.
        if pinned.is_none() {
            let chosen = profile::active();
            if chosen != wanted {
                log(&format!("now following profile '{chosen}'"));
                wanted = chosen;
            }
        }

        if reload || current != last {
            if current.is_empty() {
                log("no camera attached");
            } else {
                // Give a freshly attached camera a moment to finish enumerating
                // before writing; on an explicit reload it is already settled.
                if !reload {
                    std::thread::sleep(Duration::from_millis(700));
                }
                apply_once(dev, &wanted);
            }
            last = current;
        }
        sleep_until_reload(Duration::from_secs(interval.max(1)));
    }
}

/// One pass of the daemon's work: reread the profile, write it to the camera.
fn apply_once(dev: Option<&str>, profile_name: &str) {
    match Cam::open(dev) {
        Ok(cam) => match Profile::load(profile_name) {
            Ok(prof) => {
                let report = profile::apply(&cam, &prof);
                log(&format!(
                    "{} present; applied {} settings{}",
                    cam.name,
                    report.applied.len(),
                    if report.skipped.is_empty() {
                        String::new()
                    } else {
                        format!(", skipped {}", report.skipped.len())
                    }
                ));
                for (name, why) in &report.skipped {
                    log(&format!("  skipped {name}: {why}"));
                }
            }
            Err(e) => log(&format!("cannot load profile: {e}")),
        },
        Err(e) => log(&format!("camera not usable yet: {e}")),
    }
}

/// Sleep, but wake as soon as a reload arrives so `reload` feels immediate even
/// with a long `--interval`.
fn sleep_until_reload(total: Duration) {
    let slice = Duration::from_millis(200);
    let mut left = total;
    while !left.is_zero() && !RELOAD.load(Ordering::SeqCst) {
        let step = slice.min(left);
        std::thread::sleep(step);
        left -= step;
    }
}

fn cmd_install(dev: Option<&str>, pinned: Option<&str>, name: &str, interval: u64) -> Result<(), String> {
    if !profile::profile_path(name).exists() {
        return Err(format!("profile '{name}' does not exist yet — run `kiyoctl save` first"));
    }
    let path = service::install(pinned, dev, interval)?;
    println!("Installed login agent: {}", path.display());
    println!("Binary: {}", service::installed_binary().display());
    println!("Log:    {}", profile::config_dir().join("kiyoctl.log").display());
    match pinned {
        Some(p) => println!("\nProfile '{p}' will be reapplied at login and whenever the camera is reconnected."),
        None => println!(
            "\nThe profile in use ('{name}' right now) will be reapplied at login and whenever the \
             camera is reconnected. Choosing another one in the UI changes what the agent applies."
        ),
    }
    Ok(())
}

fn parse_assignments(args: &[String]) -> Result<Vec<(String, String)>, String> {
    // `set hdr on` is the common case; `set hdr=on brightness=140` scales up.
    if args.len() == 2 && !args[0].contains('=') {
        return Ok(vec![(args[0].to_lowercase(), args[1].clone())]);
    }
    args.iter()
        .map(|a| {
            a.split_once('=')
                .map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string()))
                .ok_or_else(|| format!("expected name=value, got '{a}'"))
        })
        .collect()
}

fn unknown_control(name: &str) -> String {
    let known: Vec<&str> = CONTROLS.iter().map(|c| c.name).collect();
    format!("unknown control '{name}'. Known controls: {}", known.join(", "))
}

fn log(msg: &str) {
    println!("[{}] {msg}", timestamp());
    use std::io::Write;
    let _ = std::io::stdout().flush();
}

fn timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, m, d) = civil_from_days((secs / 86400) as i64);
    let tod = secs % 86400;
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}Z", tod / 3600, (tod % 3600) / 60, tod % 60)
}

/// Days since the Unix epoch to a civil date (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as i64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
