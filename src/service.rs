//! Installing kiyoctl as a per-user launchd agent, so a profile is reapplied at
//! login and whenever the camera is plugged back in.

use crate::profile::config_dir;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const LABEL: &str = "local.kiyoctl";

pub fn plist_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    Path::new(&home).join("Library/LaunchAgents").join(format!("{LABEL}.plist"))
}

pub fn installed_binary() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    Path::new(&home).join(".local/bin/kiyoctl")
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn plist(binary: &Path, profile: Option<&str>, device: Option<&str>, interval: u64) -> String {
    let log = config_dir().join("kiyoctl.log");
    let mut args = vec![
        binary.display().to_string(),
        "daemon".into(),
        "--interval".into(),
        interval.to_string(),
    ];
    // Without --profile the agent follows whichever profile is in use, which is
    // what makes a choice in the UI stick.
    if let Some(p) = profile {
        args.push("--profile".into());
        args.push(p.into());
    }
    if let Some(d) = device {
        args.push("--device".into());
        args.push(d.into());
    }
    let arg_xml: String = args
        .iter()
        .map(|a| format!("    <string>{}</string>\n", xml_escape(a)))
        .collect();

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
{arg_xml}  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>ProcessType</key>
  <string>Background</string>
  <key>StandardOutPath</key>
  <string>{log}</string>
  <key>StandardErrorPath</key>
  <string>{log}</string>
</dict>
</plist>
"#,
        log = xml_escape(&log.display().to_string())
    )
}

fn launchctl(args: &[&str]) -> Result<(), String> {
    let out = Command::new("launchctl")
        .args(args)
        .output()
        .map_err(|e| format!("cannot run launchctl: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    Err(if stderr.is_empty() {
        format!("launchctl {} failed", args.join(" "))
    } else {
        stderr
    })
}

fn domain() -> String {
    format!("gui/{}", unsafe { libc_getuid() })
}

fn target() -> String {
    format!("{}/{LABEL}", domain())
}

// Avoiding a libc dependency for a single call.
extern "C" {
    #[link_name = "getuid"]
    fn libc_getuid() -> u32;
}

/// `profile` pins the agent to one profile; `None` lets it follow the profile
/// in use.
pub fn install(profile: Option<&str>, device: Option<&str>, interval: u64) -> Result<PathBuf, String> {
    // Copy the running binary somewhere stable — a launchd agent pointing at
    // target/debug would break on the next rebuild.
    let src = std::env::current_exe().map_err(|e| format!("cannot locate kiyoctl binary: {e}"))?;
    let dst = installed_binary();
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    if src != dst {
        std::fs::copy(&src, &dst).map_err(|e| format!("cannot copy binary to {}: {e}", dst.display()))?;
    }

    let path = plist_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    std::fs::write(&path, plist(&dst, profile, device, interval))
        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;

    // Replace any previous instance; booting out an absent agent is not an error.
    let _ = launchctl(&["bootout", &target()]);
    launchctl(&["bootstrap", &domain(), &path.display().to_string()])?;
    Ok(path)
}

pub fn uninstall() -> Result<(), String> {
    let _ = launchctl(&["bootout", &target()]);
    let path = plist_path();
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("cannot remove {}: {e}", path.display()))?;
    }
    Ok(())
}

/// What launchd knows about the agent, and what it was asked to watch.
pub struct Agent {
    pub state: String,
    pub pid: Option<u32>,
    pub profile: Option<String>,
    pub device: Option<String>,
    pub interval: Option<String>,
}

impl Agent {
    pub fn running(&self) -> bool {
        self.pid.is_some()
    }
}

pub fn installed() -> bool {
    plist_path().exists()
}

/// A daemon flag as recorded in the plist — what the agent will run with once
/// loaded, readable while it is stopped.
pub fn installed_flag(name: &str) -> Option<String> {
    let text = std::fs::read_to_string(plist_path()).ok()?;
    let strings: Vec<&str> = text
        .split("<string>")
        .skip(1)
        .filter_map(|s| s.split_once("</string>").map(|(v, _)| v))
        .collect();
    flag_value(&strings, name).map(xml_unescape)
}

fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

fn flag_value<'a, S: AsRef<str>>(args: &'a [S], name: &str) -> Option<&'a str> {
    let i = args.iter().position(|a| a.as_ref() == name)?;
    args.get(i + 1).map(AsRef::as_ref)
}

/// `None` when the agent is not loaded into the user's launchd domain.
pub fn agent() -> Option<Agent> {
    let out = Command::new("launchctl").args(["print", &target()]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let field = |key: &str| {
        text.lines()
            .map(str::trim)
            .find(|l| l.starts_with(&format!("{key} = ")))
            .map(|l| l[key.len() + 3..].to_string())
    };
    let args = arguments(&text);
    let flag = |name: &str| flag_value(&args, name).map(str::to_string);
    Some(Agent {
        state: field("state").unwrap_or_else(|| "loaded".into()),
        pid: field("pid").and_then(|p| p.parse().ok()),
        profile: flag("--profile"),
        device: flag("--device"),
        interval: flag("--interval"),
    })
}

/// launchd reports a just-started agent as `xpcproxy` for a moment before the
/// real program is exec'd; wait that out so `start` can print a settled state.
pub fn settle() {
    for _ in 0..20 {
        match agent() {
            Some(a) if a.state != "xpcproxy" => return,
            None => return,
            _ => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    }
}

/// The `arguments = { ... }` block of `launchctl print`, one entry per line.
fn arguments(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .skip_while(|l| *l != "arguments = {")
        .skip(1)
        .take_while(|l| *l != "}")
        .map(str::to_string)
        .collect()
}

/// Load the agent, or kick it awake if it is loaded but idle. Idempotent.
pub fn start() -> Result<(), String> {
    let path = plist_path();
    if !path.exists() {
        return Err(format!(
            "the login agent is not installed ({} does not exist) — run `kiyoctl install` first",
            path.display()
        ));
    }
    match agent() {
        Some(_) => launchctl(&["kickstart", &target()]),
        None => launchctl(&["bootstrap", &domain(), &path.display().to_string()]),
    }
}

/// Unload the agent. It comes back at the next login unless uninstalled.
pub fn stop() -> Result<(), String> {
    if agent().is_none() {
        return Err("the login agent is not loaded".into());
    }
    launchctl(&["bootout", &target()])
}

pub fn restart() -> Result<(), String> {
    // `kickstart -k` only works on a loaded agent; otherwise this is a start.
    match agent() {
        Some(_) => launchctl(&["kickstart", "-k", &target()]),
        None => start(),
    }
}

/// Ask the running daemon to re-read its profile and write it to the camera.
pub fn reload() -> Result<(), String> {
    match agent() {
        Some(a) if a.running() => launchctl(&["kill", "SIGHUP", &target()]),
        Some(_) => Err("the login agent is loaded but not running — try `kiyoctl daemon start`".into()),
        None => Err("the login agent is not running — use `kiyoctl apply` to apply the profile once".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_of(xml: &str) -> Vec<&str> {
        xml.split("<array>")
            .nth(1)
            .and_then(|s| s.split("</array>").next())
            .unwrap()
            .split("<string>")
            .skip(1)
            .filter_map(|s| s.split_once("</string>").map(|(v, _)| v))
            .collect()
    }

    #[test]
    fn an_unpinned_agent_names_no_profile_so_it_follows_the_one_in_use() {
        let xml = plist(Path::new("/bin/kiyoctl"), None, None, 2);
        let args = args_of(&xml);
        assert_eq!(args, ["/bin/kiyoctl", "daemon", "--interval", "2"]);
    }

    #[test]
    fn a_pinned_agent_carries_the_profile_and_device() {
        let xml = plist(Path::new("/bin/kiyoctl"), Some("night"), Some("1532:0e05"), 5);
        let args = args_of(&xml);
        assert_eq!(
            args,
            ["/bin/kiyoctl", "daemon", "--interval", "5", "--profile", "night", "--device", "1532:0e05"]
        );
        assert_eq!(flag_value(&args, "--profile"), Some("night"));
    }

    #[test]
    fn the_arguments_block_of_launchctl_print_is_read_back() {
        let text = "\tstate = running\n\targuments = {\n\t\t/bin/kiyoctl\n\t\tdaemon\n\t\t--profile\n\t\tnight\n\t}\n\tpid = 42\n";
        let args = arguments(text);
        assert_eq!(flag_value(&args, "--profile"), Some("night"));
        assert_eq!(flag_value(&args, "--device"), None);
    }
}
