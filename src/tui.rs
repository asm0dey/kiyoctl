//! Interactive terminal UI.
//!
//! Adjustments are written to the camera as you make them, so the effect is
//! visible live in whatever app is showing the picture. Nothing touches the
//! saved profile until you press `s`.
//!
//! Drawing is deliberately split from the hardware: [`Ui`] holds everything the
//! screen shows and knows nothing about USB, while [`App`] owns the camera and
//! performs the writes. That keeps the rendering testable without a webcam.

use crate::controls::{self, Control, Kind};
use crate::usb::Cam;
use crate::profile::{self, Profile};

use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
    MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};

/// One line in the control list.
struct Row {
    ctrl: &'static Control,
    /// Current value in human form, or None when the camera will not report it.
    value: Option<String>,
    range: Option<(i64, i64)>,
    step: i64,
    default: Option<String>,
    writable: bool,
}

impl Row {
    fn numeric(&self) -> bool {
        matches!(self.ctrl.kind, Kind::Int { .. })
    }

    fn shown(&self) -> String {
        self.value.clone().unwrap_or_else(|| "?".into())
    }
}

enum Mode {
    Browse,
    /// Typing a value directly into the selected numeric control.
    Edit { buffer: String },
    Help,
    /// Choosing which profile to work with.
    Profiles { names: Vec<String>, selected: usize },
    /// Naming a profile that does not exist yet.
    NewProfile { buffer: String },
}

/// Where a control was last drawn, so a click can be mapped back to it.
/// Columns are half-open ranges of absolute screen positions.
#[derive(Clone, Default)]
struct Hit {
    /// Index into `Ui::rows`; not the position in `Ui::hits` once scrolled.
    index: usize,
    y: u16,
    /// The value bar, for numeric controls.
    bar: Option<(u16, u16)>,
    /// Each named option, for choice controls.
    choices: Vec<(u16, u16, &'static str)>,
}

/// What the user has just asked for, decided without touching the camera so the
/// key handling can be tested on its own.
enum Action {
    None,
    Quit,
    /// Write this value to the control on the given row.
    Write { row: usize, value: String },
    SaveProfile,
    ApplyProfile,
    ResetDefaults,
    Reload,
    /// Switch to an existing profile and put it on the camera.
    SwitchProfile(String),
    /// Create a profile from the camera's current settings.
    CreateProfile(String),
}

/// Everything drawn on screen. No hardware behind it.
pub struct Ui {
    device_name: String,
    vid: u16,
    pid: u16,
    profile_name: String,
    rows: Vec<Row>,
    list: ListState,
    mode: Mode,
    status: String,
    /// A control has been changed but not yet written to the profile.
    dirty: bool,
    /// Filled in by the last render, consumed by mouse handling.
    hits: Vec<Hit>,
    /// The clickable profile name in the header: (x0, x1, y).
    profile_hit: Option<(u16, u16, u16)>,
    /// Rows of the open dialog: (y, entry index).
    menu_hits: Vec<(u16, usize)>,
    /// Pre-formatted GUIDs of extension units no Model claims.
    unclaimed: Vec<String>,
}

/// The profile dialog lists every saved profile, then an entry for making a
/// new one. Kept as a helper so the list and the click map cannot disagree.
fn profile_entries() -> Vec<String> {
    let mut names = Profile::list();
    names.push(NEW_PROFILE.to_string());
    names
}

const NEW_PROFILE: &str = "+ new profile…";

impl Ui {
    fn selected(&self) -> usize {
        self.list.selected().unwrap_or(0)
    }

    fn move_by(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let len = self.rows.len() as isize;
        let next = (self.selected() as isize + delta).rem_euclid(len);
        self.list.select(Some(next as usize));
    }

    /// Why the control on `row` cannot be changed right now, if it cannot.
    fn blocked_by(&self, row: &Row) -> Option<String> {
        let (dep, allowed) = row.ctrl.requires?;
        let current = self.rows.iter().find(|r| r.ctrl.name == dep)?.value.clone()?;
        if allowed.contains(&current.as_str()) {
            None
        } else {
            Some(format!("{dep} is {current}; set it to {}", allowed.join(" or ")))
        }
    }

    /// Refuse, with a reason in the status line, when a control cannot be
    /// changed right now. Shared by the keyboard and the mouse.
    fn guard(&mut self, index: usize) -> bool {
        let row = &self.rows[index];
        let problem = if !row.writable {
            Some(format!("{} is read-only", row.ctrl.name))
        } else {
            self.blocked_by(row)
                .map(|why| format!("{} is not adjustable now — {why}", row.ctrl.name))
        };
        match problem {
            Some(msg) => {
                self.status = msg;
                false
            }
            None => true,
        }
    }

    /// Work out the new value for a step or a cycle of the selected control.
    fn next_value(&mut self, direction: i64, big: bool) -> Option<String> {
        if self.rows.is_empty() || !self.guard(self.selected()) {
            return None;
        }
        let row = &self.rows[self.selected()];

        match &row.ctrl.kind {
            Kind::Int { .. } => {
                let Some(current) = row.value.as_ref().and_then(|v| v.parse::<i64>().ok()) else {
                    self.status = format!("{} has no readable value", row.ctrl.name);
                    return None;
                };
                let stride = row.step * if big { 10 } else { 1 };
                let mut v = current + direction * stride;
                if let Some((lo, hi)) = row.range {
                    v = v.clamp(lo, hi);
                    if v == current {
                        self.status = format!(
                            "{} is already at its {}",
                            row.ctrl.name,
                            if direction < 0 { "minimum" } else { "maximum" }
                        );
                        return None;
                    }
                }
                Some(v.to_string())
            }
            _ => {
                let options = row.ctrl.choices().unwrap_or_default();
                if options.is_empty() {
                    return None;
                }
                let current = row.value.as_deref().unwrap_or("");
                let idx = match options.iter().position(|o| *o == current) {
                    Some(p) => (p as i64 + direction).rem_euclid(options.len() as i64) as usize,
                    // Nothing known yet (a write-only control): start at an end.
                    None if direction < 0 => options.len() - 1,
                    None => 0,
                };
                Some(options[idx].to_string())
            }
        }
    }

    /// Value for a click at column `x` on a bar spanning `[x0, x1)`, snapped to
    /// the control's resolution.
    fn value_at(&self, index: usize, x: u16, x0: u16, x1: u16) -> Option<String> {
        let row = &self.rows[index];
        let (lo, hi) = row.range?;
        let width = x1.saturating_sub(x0);
        if width == 0 || hi <= lo {
            return None;
        }
        // Map the first and last cells onto the extremes, so clicking the end
        // of the bar gives exactly the minimum or maximum.
        let frac = if width > 1 {
            (x.saturating_sub(x0).min(width - 1)) as f64 / (width - 1) as f64
        } else {
            0.0
        };
        let raw = lo as f64 + frac * (hi - lo) as f64;
        let step = row.step.max(1);
        let snapped = lo + ((raw.round() as i64 - lo) + step / 2) / step * step;
        Some(snapped.clamp(lo, hi).to_string())
    }

    /// Map a click or drag onto a control.
    fn click(&mut self, x: u16, y: u16, dragging: bool) -> Action {
        let Some(hit) = self.hits.iter().find(|h| h.y == y).cloned() else {
            return Action::None;
        };
        let index = hit.index;
        // A drag only ever affects the control it started on.
        if dragging {
            if self.selected() != index {
                return Action::None;
            }
        } else {
            self.list.select(Some(index));
        }
        if !self.guard(index) {
            return Action::None;
        }
        if let Some((x0, x1)) = hit.bar {
            if x >= x0 && x < x1 {
                let value = self.value_at(index, x, x0, x1);
                // Skip a write that would not change anything — dragging across
                // a bar would otherwise flood the camera with identical writes.
                return match value {
                    Some(v) if Some(&v) != self.rows[index].value.as_ref() => {
                        Action::Write { row: index, value: v }
                    }
                    _ => Action::None,
                };
            }
        }
        // Clicking an option name selects it directly.
        for (c0, c1, name) in &hit.choices {
            if x >= *c0 && x < *c1 && Some(*name) != self.rows[index].value.as_deref() {
                return Action::Write { row: index, value: (*name).to_string() };
            }
        }
        Action::None
    }

    /// Translate a mouse event into an action.
    fn on_mouse(&mut self, ev: MouseEvent) -> Action {
        // Any click dismisses help; scrolling under it does nothing.
        if matches!(self.mode, Mode::Help) {
            if matches!(ev.kind, MouseEventKind::Down(_)) {
                self.mode = Mode::Browse;
            }
            return Action::None;
        }
        if let Mode::Profiles { selected, .. } = &mut self.mode {
            match ev.kind {
                MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                    let down = matches!(ev.kind, MouseEventKind::ScrollDown);
                    let len = match &self.mode {
                        Mode::Profiles { names, .. } => names.len(),
                        _ => return Action::None,
                    };
                    if let Mode::Profiles { selected, .. } = &mut self.mode {
                        *selected = if down {
                            (*selected + 1) % len
                        } else {
                            (*selected + len - 1) % len
                        };
                    }
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    match self.menu_hits.iter().find(|(y, _)| *y == ev.row) {
                        Some((_, index)) => {
                            let i = *index;
                            *selected = i;
                            return self.choose_profile(i);
                        }
                        // Clicking away closes the dialog.
                        None => self.mode = Mode::Browse,
                    }
                }
                _ => {}
            }
            return Action::None;
        }
        // While typing, the mouse would only confuse matters.
        if matches!(self.mode, Mode::Edit { .. } | Mode::NewProfile { .. }) {
            return Action::None;
        }

        // The profile name in the header opens the picker.
        if let (MouseEventKind::Down(MouseButton::Left), Some((x0, x1, y))) =
            (ev.kind, self.profile_hit)
        {
            if ev.row == y && ev.column >= x0 && ev.column < x1 {
                self.open_profiles();
                return Action::None;
            }
        }
        match ev.kind {
            MouseEventKind::ScrollDown => {
                self.move_by(1);
                Action::None
            }
            MouseEventKind::ScrollUp => {
                self.move_by(-1);
                Action::None
            }
            MouseEventKind::Down(MouseButton::Left) => self.click(ev.column, ev.row, false),
            MouseEventKind::Drag(MouseButton::Left) => self.click(ev.column, ev.row, true),
            _ => Action::None,
        }
    }

    fn open_profiles(&mut self) {
        let names = profile_entries();
        // Start on the profile in use, so Enter is a no-op rather than a jump.
        let selected = names.iter().position(|n| *n == self.profile_name).unwrap_or(0);
        self.mode = Mode::Profiles { names, selected };
    }

    /// Act on the highlighted entry of the profile dialog.
    fn choose_profile(&mut self, index: usize) -> Action {
        let Mode::Profiles { names, .. } = &self.mode else {
            return Action::None;
        };
        let Some(name) = names.get(index).cloned() else {
            return Action::None;
        };
        if name == NEW_PROFILE {
            self.mode = Mode::NewProfile { buffer: String::new() };
            self.status = "Name the new profile, Enter to create".into();
            return Action::None;
        }
        self.mode = Mode::Browse;
        if name == self.profile_name {
            self.status = format!("Already using '{name}'");
            return Action::None;
        }
        Action::SwitchProfile(name)
    }

    /// Keys handled while a dialog is open. Returns None when the dialog does
    /// not claim the key.
    fn dialog_key(&mut self, key: KeyEvent) -> Option<Action> {
        match &mut self.mode {
            Mode::Profiles { names, selected } => {
                let len = names.len();
                match key.code {
                    KeyCode::Down | KeyCode::Char('j') => {
                        *selected = (*selected + 1) % len;
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        *selected = (*selected + len - 1) % len;
                    }
                    KeyCode::Enter => {
                        let i = *selected;
                        return Some(self.choose_profile(i));
                    }
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('p') => {
                        self.mode = Mode::Browse;
                    }
                    _ => {}
                }
                Some(Action::None)
            }
            Mode::NewProfile { buffer } => {
                match key.code {
                    // Keep names to something that is safe as a file name.
                    KeyCode::Char(c)
                        if c.is_ascii_alphanumeric() || c == '-' || c == '_' =>
                    {
                        buffer.push(c);
                    }
                    KeyCode::Backspace => {
                        buffer.pop();
                    }
                    KeyCode::Enter => {
                        let name = buffer.trim().to_string();
                        self.mode = Mode::Browse;
                        if name.is_empty() {
                            self.status = "Cancelled".into();
                            return Some(Action::None);
                        }
                        return Some(Action::CreateProfile(name));
                    }
                    KeyCode::Esc => {
                        self.mode = Mode::Browse;
                        self.status = "Cancelled".into();
                    }
                    _ => {}
                }
                Some(Action::None)
            }
            _ => None,
        }
    }

    /// Translate a keypress into an action, updating purely visual state.
    fn on_key(&mut self, key: KeyEvent) -> Action {
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);

        if let Some(action) = self.dialog_key(key) {
            return action;
        }

        // Edit mode swallows most keys.
        if let Mode::Edit { buffer } = &mut self.mode {
            match key.code {
                KeyCode::Char(c) if c.is_ascii_digit() || c == '-' => buffer.push(c),
                KeyCode::Backspace => {
                    buffer.pop();
                }
                KeyCode::Enter => {
                    let text = buffer.trim().to_string();
                    self.mode = Mode::Browse;
                    if text.is_empty() {
                        self.status = "Cancelled".into();
                        return Action::None;
                    }
                    let row = self.selected();
                    if let Some(why) = self.blocked_by(&self.rows[row]) {
                        self.status =
                            format!("{} is not adjustable now — {why}", self.rows[row].ctrl.name);
                        return Action::None;
                    }
                    return Action::Write { row, value: text };
                }
                KeyCode::Esc => {
                    self.mode = Mode::Browse;
                    self.status = "Cancelled".into();
                }
                _ => {}
            }
            return Action::None;
        }

        // Any key dismisses help.
        if matches!(self.mode, Mode::Help) {
            self.mode = Mode::Browse;
            return Action::None;
        }

        let step = |ui: &mut Ui, dir: i64, big: bool| match ui.next_value(dir, big) {
            Some(value) => Action::Write { row: ui.selected(), value },
            None => Action::None,
        };

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Action::Quit,
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_by(1);
                Action::None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_by(-1);
                Action::None
            }
            KeyCode::PageDown => {
                self.move_by(10);
                Action::None
            }
            KeyCode::PageUp => {
                self.move_by(-10);
                Action::None
            }
            KeyCode::Home => {
                self.list.select(Some(0));
                Action::None
            }
            KeyCode::End => {
                self.list.select(Some(self.rows.len().saturating_sub(1)));
                Action::None
            }
            KeyCode::Left | KeyCode::Char('h') => step(self, -1, shift),
            KeyCode::Right | KeyCode::Char('l') => step(self, 1, shift),
            KeyCode::Char('H') => step(self, -1, true),
            KeyCode::Char('L') => step(self, 1, true),
            KeyCode::Enter => {
                if self.rows.is_empty() {
                    return Action::None;
                }
                let i = self.selected();
                if self.rows[i].numeric() {
                    self.mode = Mode::Edit { buffer: String::new() };
                    self.status =
                        format!("Type a value for {}, Enter to confirm", self.rows[i].ctrl.name);
                    Action::None
                } else {
                    step(self, 1, false)
                }
            }
            KeyCode::Char('d') => {
                let i = self.selected();
                match self.rows[i].default.clone() {
                    Some(value) => Action::Write { row: i, value },
                    None => {
                        self.status =
                            format!("{} has no reported default", self.rows[i].ctrl.name);
                        Action::None
                    }
                }
            }
            KeyCode::Char('s') => Action::SaveProfile,
            KeyCode::Char('a') => Action::ApplyProfile,
            KeyCode::Char('R') => Action::ResetDefaults,
            KeyCode::Char('r') => Action::Reload,
            KeyCode::Char('p') => {
                self.open_profiles();
                Action::None
            }
            KeyCode::Char('?') => {
                self.mode = Mode::Help;
                Action::None
            }
            _ => Action::None,
        }
    }
}

/// The UI plus the camera it drives.
pub struct App {
    cam: Cam,
    profile: Profile,
    ui: Ui,
    quit: bool,
}

impl App {
    pub fn new(cam: Cam, profile_name: &str) -> Result<App, String> {
        let mut profile = Profile::load_or_default(profile_name)?;
        // The session edits this profile against the camera in front of us, so
        // an opaque value lands in that camera's section and not the one the
        // file happens to name.
        profile.home_to(&cam);
        let ui = Ui {
            device_name: cam.name.clone(),
            vid: cam.vid,
            pid: cam.pid,
            profile_name: profile_name.to_string(),
            rows: Vec::new(),
            list: ListState::default().with_selected(Some(0)),
            mode: Mode::Browse,
            status: format!("Profile '{profile_name}'. Press ? for keys."),
            dirty: false,
            hits: Vec::new(),
            profile_hit: None,
            menu_hits: Vec::new(),
            unclaimed: cam
                .unclaimed_units()
                .iter()
                .map(crate::usb::format_guid)
                .collect(),
        };
        let mut app = App { cam, profile, ui, quit: false };
        app.reload()?;
        Ok(app)
    }

    /// Re-read every control the camera actually implements.
    fn reload(&mut self) -> Result<(), String> {
        let mut rows = Vec::new();
        for ctrl in self.cam.controls() {
            if ctrl.is_opaque() {
                // Write-only: the best we can show is what the profile recalls.
                rows.push(Row {
                    ctrl,
                    value: self.profile.get(ctrl.name),
                    range: None,
                    step: 1,
                    default: None,
                    writable: true,
                });
                continue;
            }
            if let Some(r) = controls::read(&self.cam, ctrl)? {
                rows.push(Row {
                    ctrl,
                    value: Some(r.value),
                    range: r.range,
                    step: r.step.filter(|s| *s > 0).unwrap_or(1),
                    default: r.default,
                    writable: r.writable,
                });
            }
        }
        // Refuse to open a UI onto a camera that answers nothing.
        if !rows.iter().any(|r| !r.ctrl.is_opaque()) && !self.cam.responding {
            return Err(crate::usb::NOT_RESPONDING.into());
        }
        self.ui.rows = rows;
        Ok(())
    }

    /// Refresh the readable controls, since one control can move another.
    fn refresh_values(&mut self) {
        for row in &mut self.ui.rows {
            if row.ctrl.is_opaque() {
                continue;
            }
            if let Ok(Some(r)) = controls::read(&self.cam, row.ctrl) {
                row.value = Some(r.value);
                row.writable = r.writable;
            }
        }
    }

    fn write(&mut self, index: usize, value: String) {
        let ctrl = self.ui.rows[index].ctrl;
        match controls::write(&self.cam, ctrl, &value) {
            Ok(()) => {
                if ctrl.is_opaque() {
                    self.ui.rows[index].value = Some(value.clone());
                } else {
                    self.refresh_values();
                }
                if matches!(ctrl.unit, crate::usb::Unit::Extension(_)) {
                    // A TUI edit is one operation, like one CLI invocation.
                    self.cam.persist();
                }
                self.profile.set(ctrl.name, &value);
                self.ui.dirty = true;
                self.ui.status = format!("{} = {}", ctrl.name, value);
            }
            Err(e) => self.ui.status = self.cam.explain(e),
        }
    }

    fn save_profile(&mut self) {
        // Capture the readable controls too, so the profile is complete rather
        // than only holding what was touched this session.
        let mut prof = self.profile.clone();
        profile::capture(&self.cam, &mut prof);
        match prof.save(&self.ui.profile_name) {
            Ok(path) => {
                self.profile = prof;
                self.ui.dirty = false;
                self.ui.status = format!("Saved to {}", path.display());
            }
            Err(e) => self.ui.status = e,
        }
    }

    fn apply_profile(&mut self) {
        match Profile::load(&self.ui.profile_name) {
            Ok(prof) => {
                let report = profile::apply(&self.cam, &prof);
                self.profile = prof;
                self.profile.home_to(&self.cam);
                self.refresh_values();
                // Reflect remembered extension-unit values back into the list.
                for row in &mut self.ui.rows {
                    if row.ctrl.is_opaque() {
                        if let Some(v) = self.profile.get(row.ctrl.name) {
                            row.value = Some(v);
                        }
                    }
                }
                self.ui.dirty = false;
                self.ui.status = format!(
                    "Applied {} settings{}",
                    report.applied.len(),
                    if report.skipped.is_empty() {
                        String::new()
                    } else {
                        format!(", skipped {}", report.skipped.len())
                    }
                );
            }
            Err(e) => self.ui.status = e,
        }
    }

    /// Adopt an existing profile and put it on the camera.
    fn switch_profile(&mut self, name: String) {
        match Profile::load(&name) {
            Ok(prof) => {
                self.profile = prof;
                self.ui.profile_name = name;
                self.apply_profile();
                let applied = self.ui.status.clone();
                self.ui.status = format!("Switched to '{}' — {applied}", self.ui.profile_name);
                self.remember_choice();
            }
            Err(e) => self.ui.status = e,
        }
    }

    /// Make this the profile later commands and the login agent use. Failing to
    /// record it is worth saying, but does not undo the switch itself.
    fn remember_choice(&mut self) {
        if let Err(e) = profile::set_active(&self.ui.profile_name) {
            self.ui.status = format!("{} (but {e})", self.ui.status);
        }
    }

    /// Create a profile from what the camera is set to now, carrying over the
    /// write-only values that only kiyoctl knows about.
    fn create_profile(&mut self, name: String) {
        if profile::profile_path(&name).exists() {
            self.ui.status = format!("'{name}' already exists — pick another name");
            return;
        }
        let mut prof = self.profile.clone();
        profile::capture(&self.cam, &mut prof);
        match prof.save(&name) {
            Ok(_) => {
                self.profile = prof;
                self.ui.profile_name = name;
                self.ui.dirty = false;
                self.ui.status = format!("Created '{}' from the current settings", self.ui.profile_name);
                self.remember_choice();
            }
            Err(e) => self.ui.status = e,
        }
    }

    fn reset_defaults(&mut self) {
        let mut n = 0;
        for i in 0..self.ui.rows.len() {
            if self.ui.rows[i].ctrl.is_opaque() || !self.ui.rows[i].writable {
                continue;
            }
            if let Some(d) = self.ui.rows[i].default.clone() {
                if controls::write(&self.cam, self.ui.rows[i].ctrl, &d).is_ok() {
                    self.profile.set(self.ui.rows[i].ctrl.name, &d);
                    n += 1;
                }
            }
        }
        self.refresh_values();
        self.ui.dirty = true;
        self.ui.status = format!("Restored {n} controls to camera defaults");
    }

    fn on_mouse(&mut self, ev: MouseEvent) {
        let action = self.ui.on_mouse(ev);
        self.dispatch(action);
    }

    fn on_key(&mut self, key: KeyEvent) {
        let action = self.ui.on_key(key);
        self.dispatch(action);
    }

    fn dispatch(&mut self, action: Action) {
        match action {
            Action::None => {}
            Action::Quit => self.quit = true,
            Action::Write { row, value } => self.write(row, value),
            Action::SaveProfile => self.save_profile(),
            Action::ApplyProfile => self.apply_profile(),
            Action::ResetDefaults => self.reset_defaults(),
            Action::Reload => match self.reload() {
                Ok(()) => self.ui.status = "Re-read from camera".into(),
                Err(e) => self.ui.status = e,
            },
            Action::SwitchProfile(name) => self.switch_profile(name),
            Action::CreateProfile(name) => self.create_profile(name),
        }
    }
}

/// Draw a proportional bar for a numeric control.
fn bar(value: i64, lo: i64, hi: i64, width: usize) -> String {
    if width == 0 || hi <= lo {
        return String::new();
    }
    let frac = ((value - lo) as f64 / (hi - lo) as f64).clamp(0.0, 1.0);
    let filled = (frac * width as f64).round() as usize;
    (0..width).map(|i| if i < filled { '█' } else { '·' }).collect()
}

fn render(frame: &mut Frame, ui: &mut Ui) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(3),
        Constraint::Length(4),
    ])
    .areas(frame.area());

    // --- header ---
    // Track widths as the spans are built, so the profile name stays clickable
    // wherever it lands.
    let lead = format!(
        "{}  {:04x}:{:04x}   profile: ",
        ui.device_name, ui.vid, ui.pid
    );
    let profile_x0 = header.x + 1 + lead.chars().count() as u16;
    ui.profile_hit = Some((
        profile_x0,
        profile_x0 + ui.profile_name.chars().count() as u16,
        header.y + 1,
    ));

    let title = Line::from(vec![
        Span::styled(
            ui.device_name.clone(),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!("  {:04x}:{:04x}", ui.vid, ui.pid)),
        Span::raw("   profile: "),
        Span::styled(
            ui.profile_name.clone(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::UNDERLINED),
        ),
        Span::styled(
            if ui.dirty { "  * unsaved" } else { "" },
            Style::default().fg(Color::Yellow),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(title).block(Block::default().borders(Borders::ALL).title(" kiyoctl ")),
        header,
    );

    // --- control list ---
    let name_w = ui.rows.iter().map(|r| r.ctrl.name.len()).max().unwrap_or(10);
    let value_w = ui.rows.iter().map(|r| r.shown().len()).max().unwrap_or(6).max(6);
    // Whatever is left after borders, name, value and padding goes to the bar.
    let bar_w = (body.width as usize)
        .saturating_sub(name_w + value_w + 24)
        .clamp(0, 32);

    let editing = matches!(ui.mode, Mode::Edit { .. });
    let edit_buffer = match &ui.mode {
        Mode::Edit { buffer } => buffer.clone(),
        _ => String::new(),
    };
    let selected = ui.list.selected();

    // Column where the bar or the choice names begin, relative to the list's
    // inner area. Mirrors the spans built below.
    let value_x = 3 + name_w + 2 + value_w + 2;
    let mut layout: Vec<Hit> = Vec::with_capacity(ui.rows.len());

    let items: Vec<ListItem> = ui
        .rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let is_selected = selected == Some(i);
            let blocked = ui.blocked_by(row).is_some();

            let name_style = if !row.writable || blocked {
                Style::default().fg(Color::DarkGray)
            } else if row.ctrl.is_opaque() {
                Style::default().fg(Color::Magenta)
            } else {
                Style::default()
            };

            let value_text = if is_selected && editing {
                format!("{edit_buffer}_")
            } else {
                row.shown()
            };
            let value_style = if is_selected && editing {
                Style::default().fg(Color::Black).bg(Color::Yellow)
            } else if row.value.is_none() {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            };

            let mut spans = vec![
                Span::raw(if is_selected { " > " } else { "   " }),
                Span::styled(format!("{:<name_w$}", row.ctrl.name), name_style),
                Span::raw("  "),
                Span::styled(format!("{value_text:>value_w$}"), value_style),
                Span::raw("  "),
            ];

            let mut hit = Hit::default();
            let numeric_value = row.value.as_ref().and_then(|v| v.parse::<i64>().ok());
            if let (Some((lo, hi)), Some(v)) = (row.range, numeric_value) {
                spans.push(Span::styled(
                    bar(v, lo, hi, bar_w),
                    Style::default().fg(if blocked { Color::DarkGray } else { Color::Blue }),
                ));
                spans.push(Span::styled(
                    format!("  {lo}..{hi}"),
                    Style::default().fg(Color::DarkGray),
                ));
                if bar_w > 0 {
                    hit.bar = Some((value_x as u16, (value_x + bar_w) as u16));
                }
            } else if let Some(choices) = row.ctrl.choices() {
                // Show the options, marking the active one.
                let current = row.shown();
                let mut x = value_x;
                for c in choices {
                    let style = if c == current {
                        Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };
                    spans.push(Span::styled(c.to_string(), style));
                    spans.push(Span::raw(" "));
                    hit.choices.push((x as u16, (x + c.len()) as u16, c));
                    x += c.len() + 1;
                }
            }
            layout.push(hit);

            if blocked {
                spans.push(Span::styled(" (locked)", Style::default().fg(Color::Red)));
            }

            let style = if is_selected {
                Style::default().bg(Color::Rgb(40, 40, 55))
            } else {
                Style::default()
            };
            ListItem::new(Line::from(spans)).style(style)
        })
        .collect();

    let opaque_note = if ui.rows.iter().any(|r| r.ctrl.is_opaque()) {
        " (magenta = write-only, remembered by kiyoctl) "
    } else {
        " "
    };
    frame.render_stateful_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" controls{opaque_note}")),
        ),
        body,
        &mut ui.list,
    );

    // Translate the layout into absolute screen positions for mouse handling.
    // Read the scroll offset only after rendering, since the list adjusts it to
    // keep the selection visible.
    let inner = body.inner(Margin::new(1, 1));
    let offset = ui.list.offset();
    ui.hits = layout
        .into_iter()
        .enumerate()
        .skip(offset)
        .take(inner.height as usize)
        .map(|(i, mut hit)| {
            hit.index = i;
            hit.y = inner.y + (i - offset) as u16;
            if let Some((x0, x1)) = hit.bar {
                hit.bar = Some((inner.x + x0, inner.x + x1));
            }
            for c in &mut hit.choices {
                c.0 += inner.x;
                c.1 += inner.x;
            }
            hit
        })
        .collect();

    // --- footer: status above, keys below ---
    frame.render_widget(Block::default().borders(Borders::ALL), footer);
    let inner = footer.inner(Margin::new(1, 1));
    let [status_area, keys_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(inner);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            ui.status.clone(),
            Style::default().fg(Color::Yellow),
        ))),
        status_area,
    );
    // Drop to a shorter hint rather than let the line truncate mid-word.
    let keys = if editing {
        "digits · Enter confirm · Esc cancel"
    } else if keys_area.width >= 92 {
        "↑↓ move · ←→ adjust · Enter set · p profile · s save · a apply · R reset · ? help · q quit"
    } else if keys_area.width >= 60 {
        "↑↓ move · ←→ adjust · p profile · s save · ? help · q quit"
    } else {
        "? help · q quit"
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            keys,
            Style::default().fg(Color::DarkGray),
        ))),
        keys_area,
    );

    ui.menu_hits.clear();
    match &ui.mode {
        Mode::Help => render_help(frame, frame.area(), ui),
        Mode::Profiles { names, selected } => {
            let hits = render_profiles(frame, frame.area(), names, *selected);
            ui.menu_hits = hits;
        }
        Mode::NewProfile { buffer } => render_new_profile(frame, frame.area(), buffer),
        _ => {}
    }
}

/// Centre a box of the given size inside `area`.
fn centred(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width.saturating_sub(2));
    let h = h.min(area.height.saturating_sub(2));
    Rect {
        x: area.x + area.width.saturating_sub(w) / 2,
        y: area.y + area.height.saturating_sub(h) / 2,
        width: w,
        height: h,
    }
}

/// Draw the profile picker, returning where each entry landed.
fn render_profiles(
    frame: &mut Frame,
    area: Rect,
    names: &[String],
    selected: usize,
) -> Vec<(u16, usize)> {
    let width = names
        .iter()
        .map(|n| n.chars().count())
        .max()
        .unwrap_or(10)
        .max(28) as u16
        + 6;
    let popup = centred(area, width, names.len() as u16 + 4);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::default().borders(Borders::ALL).title(" profiles "),
        popup,
    );

    let inner = popup.inner(Margin::new(2, 1));
    let mut hits = Vec::new();
    for (i, name) in names.iter().enumerate() {
        if i as u16 >= inner.height.saturating_sub(1) {
            break;
        }
        let y = inner.y + i as u16;
        let is_new = name == NEW_PROFILE;
        let style = if i == selected {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else if is_new {
            Style::default().fg(Color::Green)
        } else {
            Style::default()
        };
        let marker = if i == selected { " > " } else { "   " };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(format!("{marker}{name}"), style))),
            Rect { x: inner.x, y, width: inner.width, height: 1 },
        );
        hits.push((y, i));
    }

    // Footer hint on the last inner line.
    let hint_y = popup.y + popup.height - 2;
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Enter choose · Esc close",
            Style::default().fg(Color::DarkGray),
        ))),
        Rect { x: inner.x, y: hint_y, width: inner.width, height: 1 },
    );
    hits
}

fn render_new_profile(frame: &mut Frame, area: Rect, buffer: &str) {
    let popup = centred(area, 44, 5);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::default().borders(Borders::ALL).title(" new profile "),
        popup,
    );
    let inner = popup.inner(Margin::new(2, 1));
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::raw("name: "),
                Span::styled(
                    format!("{buffer}_"),
                    Style::default().fg(Color::Black).bg(Color::Yellow),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Enter create · Esc cancel",
                Style::default().fg(Color::DarkGray),
            )),
        ]),
        inner,
    );
}

fn render_help(frame: &mut Frame, area: Rect, ui: &Ui) {
    // Kept short enough to fit an 80x24 terminal without clipping.
    let mut text = vec![
        Line::from(Span::styled(
            "kiyoctl keys",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("  ↑ ↓ / j k        move between controls"),
        Line::from("  ← → / h l        step a number, cycle a choice"),
        Line::from("  ⇧ + ← →          step by ten"),
        Line::from("  Enter            type an exact number"),
        Line::from("  d                this control's camera default"),
        Line::from("  mouse            click a bar or option; drag to sweep"),
        Line::from(""),
        Line::from("  s  save profile      a  apply profile"),
        Line::from("  R  reset defaults    r  re-read from camera"),
        Line::from("  p  switch profile    q  quit"),
        Line::from(""),
        Line::from(Span::styled(
            "  Changes reach the camera at once; press s to keep them.",
            Style::default().fg(Color::Green),
        )),
        Line::from(Span::styled(
            "  Magenta controls are write-only — kiyoctl remembers those.",
            Style::default().fg(Color::Magenta),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  press any key to close",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    if !ui.unclaimed.is_empty() {
        text.push(Line::from(""));
        text.push(Line::from(format!(
            "This camera has {} unrecognised extension unit(s):",
            ui.unclaimed.len()
        )));
        for guid in &ui.unclaimed {
            text.push(Line::from(format!("  {guid}")));
        }
        text.push(Line::from("Run `kiyoctl probe`, then see docs/adding-a-camera.md"));
    }

    let w = 63.min(area.width.saturating_sub(4));
    let h = (text.len() as u16 + 2).min(area.height.saturating_sub(2));
    let popup = Rect {
        x: area.x + area.width.saturating_sub(w) / 2,
        y: area.y + area.height.saturating_sub(h) / 2,
        width: w,
        height: h,
    };
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title(" help ")),
        popup,
    );
}

pub fn run(cam: Cam, profile_name: &str) -> Result<(), String> {
    let mut app = App::new(cam, profile_name)?;
    let mut terminal: DefaultTerminal = ratatui::init();
    // Mouse reporting is not part of ratatui's default setup, and must be
    // switched off again or the host terminal keeps emitting escape sequences.
    let mouse = execute!(std::io::stdout(), EnableMouseCapture).is_ok();
    let result = event_loop(&mut terminal, &mut app);
    if mouse {
        let _ = execute!(std::io::stdout(), DisableMouseCapture);
    }
    ratatui::restore();
    result
}

fn event_loop(terminal: &mut DefaultTerminal, app: &mut App) -> Result<(), String> {
    loop {
        terminal
            .draw(|frame| render(frame, &mut app.ui))
            .map_err(|e| format!("draw failed: {e}"))?;

        match event::read().map_err(|e| format!("input failed: {e}"))? {
            event::Event::Key(key) if key.kind == KeyEventKind::Press => app.on_key(key),
            event::Event::Mouse(ev) => app.on_mouse(ev),
            _ => {}
        }
        if app.quit {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn row(name: &str, value: Option<&str>, range: Option<(i64, i64)>, step: i64) -> Row {
        Row {
            ctrl: controls::find_any(name).expect("known control"),
            value: value.map(str::to_string),
            range,
            step,
            default: Some("128".into()),
            writable: true,
        }
    }

    /// A fixed camera-shaped state, so snapshots never depend on hardware.
    fn ui() -> Ui {
        Ui {
            device_name: "Razer Kiyo Pro".into(),
            vid: 0x1532,
            pid: 0x0e05,
            profile_name: "default".into(),
            rows: vec![
                row("brightness", Some("129"), Some((0, 255)), 1),
                row("contrast", Some("163"), Some((0, 255)), 1),
                row("white_balance_auto", Some("on"), None, 1),
                // Locked: white balance needs auto white balance off.
                row("white_balance", Some("4430"), Some((2000, 7500)), 10),
                row("hdr", Some("on"), None, 1),
                row("hdr_mode", None, None, 1),
                row("fov", Some("wide"), None, 1),
            ],
            list: ListState::default().with_selected(Some(0)),
            mode: Mode::Browse,
            status: "Profile 'default'. Press ? for keys.".into(),
            dirty: false,
            hits: Vec::new(),
            profile_hit: None,
            menu_hits: Vec::new(),
            unclaimed: Vec::new(),
        }
    }

    /// Mouse handling needs real geometry, so lay the screen out first.
    fn laid_out() -> Ui {
        let mut ui = ui();
        draw(&mut ui);
        ui
    }

    fn click_at(ui: &mut Ui, x: u16, y: u16) -> Action {
        ui.on_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        })
    }

    fn scroll(ui: &mut Ui, up: bool) -> Action {
        ui.on_mouse(MouseEvent {
            kind: if up { MouseEventKind::ScrollUp } else { MouseEventKind::ScrollDown },
            column: 10,
            row: 5,
            modifiers: KeyModifiers::NONE,
        })
    }

    /// The row for a control, as drawn.
    fn hit_of(ui: &Ui, name: &str) -> Hit {
        let index = ui.rows.iter().position(|r| r.ctrl.name == name).expect("row");
        ui.hits.iter().find(|h| h.index == index).expect("drawn").clone()
    }

    fn draw_at(ui: &mut Ui, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal.draw(|f| render(f, ui)).expect("draw");
        format!("{}", terminal.backend())
    }

    fn draw(ui: &mut Ui) -> String {
        draw_at(ui, 100, 22)
    }

    fn press(ui: &mut Ui, code: KeyCode) -> Action {
        ui.on_key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn browse_view() {
        assert_snapshot!(draw(&mut ui()));
    }

    #[test]
    fn help_overlay() {
        let mut ui = ui();
        press(&mut ui, KeyCode::Char('?'));
        assert_snapshot!(draw(&mut ui));
    }

    /// The smallest terminal worth supporting: the key hints must fall back to
    /// a shorter list instead of being cut off mid-word.
    #[test]
    fn narrow_terminal() {
        assert_snapshot!(draw_at(&mut ui(), 80, 24));
    }

    /// Help has to fit an 80x24 terminal, closing hint included.
    #[test]
    fn help_overlay_in_a_small_terminal() {
        let mut ui = ui();
        press(&mut ui, KeyCode::Char('?'));
        assert_snapshot!(draw_at(&mut ui, 80, 24));
    }

    #[test]
    fn edit_mode_shows_typed_digits() {
        let mut ui = ui();
        press(&mut ui, KeyCode::Enter);
        for c in "205".chars() {
            press(&mut ui, KeyCode::Char(c));
        }
        assert_snapshot!(draw(&mut ui));
    }

    #[test]
    fn unsaved_marker_and_selection() {
        let mut ui = ui();
        ui.dirty = true;
        ui.move_by(4);
        ui.status = "hdr = on".into();
        assert_snapshot!(draw(&mut ui));
    }

    // --- behaviour, independent of how it is drawn ---

    #[test]
    fn arrows_step_numeric_controls_and_clamp() {
        let mut ui = ui();
        // brightness 129 +1
        match press(&mut ui, KeyCode::Right) {
            Action::Write { row, value } => {
                assert_eq!(row, 0);
                assert_eq!(value, "130");
            }
            _ => panic!("expected a write"),
        }
        // Shift steps by ten times the resolution.
        let big = ui.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT));
        assert!(matches!(big, Action::Write { value, .. } if value == "139"));

        // At the ceiling it refuses rather than sending a doomed write.
        ui.rows[0].value = Some("255".into());
        assert!(matches!(press(&mut ui, KeyCode::Right), Action::None));
        assert!(ui.status.contains("maximum"));
    }

    #[test]
    fn choices_cycle_in_both_directions() {
        let mut ui = ui();
        ui.move_by(6); // fov = wide
        assert!(matches!(press(&mut ui, KeyCode::Right), Action::Write { value, .. } if value == "medium"));
        assert!(matches!(press(&mut ui, KeyCode::Left), Action::Write { value, .. } if value == "narrow"));
    }

    #[test]
    fn write_only_control_starts_from_an_end() {
        let mut ui = ui();
        ui.move_by(5); // hdr_mode, never read back
        assert!(matches!(press(&mut ui, KeyCode::Right), Action::Write { value, .. } if value == "dark"));
    }

    #[test]
    fn locked_control_is_refused_with_a_reason() {
        let mut ui = ui();
        ui.move_by(3); // white_balance, while auto white balance is on
        assert!(matches!(press(&mut ui, KeyCode::Right), Action::None));
        assert!(ui.status.contains("white_balance_auto is on"));
    }

    #[test]
    fn typed_value_is_written_on_enter() {
        let mut ui = ui();
        press(&mut ui, KeyCode::Enter);
        for c in "200".chars() {
            press(&mut ui, KeyCode::Char(c));
        }
        assert!(matches!(press(&mut ui, KeyCode::Enter), Action::Write { value, .. } if value == "200"));
    }

    #[test]
    fn escape_cancels_an_edit_without_writing() {
        let mut ui = ui();
        press(&mut ui, KeyCode::Enter);
        press(&mut ui, KeyCode::Char('9'));
        assert!(matches!(press(&mut ui, KeyCode::Esc), Action::None));
        assert!(matches!(ui.mode, Mode::Browse));
        assert_eq!(ui.rows[0].value.as_deref(), Some("129"));
    }

    #[test]
    fn navigation_wraps_around() {
        let mut ui = ui();
        press(&mut ui, KeyCode::Up);
        assert_eq!(ui.selected(), 6);
        press(&mut ui, KeyCode::Down);
        assert_eq!(ui.selected(), 0);
    }

    /// Exercises the real device path — reading every control over USB and
    /// drawing the result. Values vary with camera state, so this asserts
    /// structure rather than a snapshot. Run with `cargo test -- --ignored`
    /// while a camera is attached.
    #[test]
    #[ignore = "requires an attached camera"]
    fn renders_a_real_camera() {
        let cam = Cam::open(None).expect("a camera should be attached");
        let mut app = App::new(cam, "default").expect("app builds");
        assert!(!app.ui.rows.is_empty(), "no controls were read");

        let screen = draw_at(&mut app.ui, 100, 30);
        assert!(screen.contains("brightness"), "brightness missing:\n{screen}");
        println!("{screen}");
    }

    // --- profile dialog ---

    #[test]
    fn clicking_the_profile_name_opens_the_picker() {
        let mut ui = laid_out();
        let (x0, _, y) = ui.profile_hit.expect("profile name is clickable");
        click_at(&mut ui, x0, y);
        assert!(matches!(ui.mode, Mode::Profiles { .. }));
    }

    #[test]
    fn the_clickable_region_covers_the_name_and_no_more() {
        let mut ui = laid_out();
        let (x0, x1, y) = ui.profile_hit.expect("hit");
        // Width matches the name that is drawn.
        assert_eq!((x1 - x0) as usize, ui.profile_name.chars().count());
        // Just before the name is the label, which must not open the dialog.
        click_at(&mut ui, x0 - 1, y);
        assert!(matches!(ui.mode, Mode::Browse));
        click_at(&mut ui, x1 - 1, y);
        assert!(matches!(ui.mode, Mode::Profiles { .. }));
    }

    #[test]
    fn p_opens_the_picker_and_esc_closes_it() {
        let mut ui = laid_out();
        press(&mut ui, KeyCode::Char('p'));
        assert!(matches!(ui.mode, Mode::Profiles { .. }));
        press(&mut ui, KeyCode::Esc);
        assert!(matches!(ui.mode, Mode::Browse));
    }

    /// The picker must start on the profile in use, whatever the order on disk.
    #[test]
    fn picker_starts_on_the_current_profile() {
        let mut ui = ui();
        ui.profile_name = "beta".into();
        ui.mode = Mode::Profiles {
            names: vec!["alpha".into(), "beta".into(), NEW_PROFILE.into()],
            selected: 0,
        };
        // Re-open the way the UI does, to exercise the lookup.
        ui.open_profiles();
        if let Mode::Profiles { names, selected } = &ui.mode {
            // Only meaningful when "beta" is actually on disk; otherwise the
            // fallback of 0 applies.
            if let Some(i) = names.iter().position(|n| n == "beta") {
                assert_eq!(*selected, i);
            }
        } else {
            panic!("dialog did not open");
        }
    }

    #[test]
    fn choosing_another_profile_switches_to_it() {
        let mut ui = ui();
        ui.mode = Mode::Profiles {
            names: vec!["default".into(), "streaming".into(), NEW_PROFILE.into()],
            selected: 1,
        };
        match ui.choose_profile(1) {
            Action::SwitchProfile(name) => assert_eq!(name, "streaming"),
            _ => panic!("expected a switch"),
        }
        assert!(matches!(ui.mode, Mode::Browse));
    }

    #[test]
    fn choosing_the_current_profile_does_nothing() {
        let mut ui = ui(); // profile_name is "default"
        ui.mode = Mode::Profiles {
            names: vec!["default".into(), NEW_PROFILE.into()],
            selected: 0,
        };
        assert!(matches!(ui.choose_profile(0), Action::None));
        assert!(ui.status.contains("Already using"));
    }

    #[test]
    fn the_new_entry_asks_for_a_name() {
        let mut ui = ui();
        ui.mode = Mode::Profiles {
            names: vec!["default".into(), NEW_PROFILE.into()],
            selected: 1,
        };
        assert!(matches!(ui.choose_profile(1), Action::None));
        assert!(matches!(ui.mode, Mode::NewProfile { .. }));

        for c in "night-shoot".chars() {
            press(&mut ui, KeyCode::Char(c));
        }
        match press(&mut ui, KeyCode::Enter) {
            Action::CreateProfile(name) => assert_eq!(name, "night-shoot"),
            _ => panic!("expected a create"),
        }
    }

    #[test]
    fn new_profile_names_reject_path_characters() {
        let mut ui = ui();
        ui.mode = Mode::NewProfile { buffer: String::new() };
        for c in "../etc/pw n".chars() {
            press(&mut ui, KeyCode::Char(c));
        }
        match press(&mut ui, KeyCode::Enter) {
            // Only the safe characters survive.
            Action::CreateProfile(name) => assert_eq!(name, "etcpwn"),
            _ => panic!("expected a create"),
        }
    }

    #[test]
    fn an_empty_new_name_is_cancelled() {
        let mut ui = ui();
        ui.mode = Mode::NewProfile { buffer: String::new() };
        assert!(matches!(press(&mut ui, KeyCode::Enter), Action::None));
        assert!(matches!(ui.mode, Mode::Browse));
    }

    #[test]
    fn clicking_a_picker_entry_chooses_it() {
        let mut ui = ui();
        ui.mode = Mode::Profiles {
            names: vec!["default".into(), "streaming".into(), NEW_PROFILE.into()],
            selected: 0,
        };
        draw(&mut ui);
        let (y, _) = ui.menu_hits[1];
        match click_at(&mut ui, 20, y) {
            Action::SwitchProfile(name) => assert_eq!(name, "streaming"),
            _ => panic!("expected a switch"),
        }
    }

    #[test]
    fn clicking_away_closes_the_picker() {
        let mut ui = ui();
        ui.mode = Mode::Profiles {
            names: vec!["default".into(), NEW_PROFILE.into()],
            selected: 0,
        };
        draw(&mut ui);
        // The very top line is outside the dialog.
        click_at(&mut ui, 1, 0);
        assert!(matches!(ui.mode, Mode::Browse));
    }

    #[test]
    fn picker_navigation_wraps() {
        let mut ui = ui();
        ui.mode = Mode::Profiles {
            names: vec!["a".into(), "b".into(), NEW_PROFILE.into()],
            selected: 0,
        };
        press(&mut ui, KeyCode::Up);
        assert!(matches!(&ui.mode, Mode::Profiles { selected, .. } if *selected == 2));
        press(&mut ui, KeyCode::Down);
        assert!(matches!(&ui.mode, Mode::Profiles { selected, .. } if *selected == 0));
    }

    #[test]
    fn picker_dialog_renders() {
        let mut ui = ui();
        ui.mode = Mode::Profiles {
            names: vec!["default".into(), "streaming".into(), NEW_PROFILE.into()],
            selected: 1,
        };
        assert_snapshot!(draw(&mut ui));
    }

    #[test]
    fn new_profile_dialog_renders() {
        let mut ui = ui();
        ui.mode = Mode::NewProfile { buffer: "night-shoot".into() };
        assert_snapshot!(draw(&mut ui));
    }

    // --- mouse ---

    #[test]
    fn clicking_a_row_selects_it() {
        let mut ui = laid_out();
        let hit = hit_of(&ui, "fov");
        assert!(matches!(click_at(&mut ui, 4, hit.y), Action::None));
        assert_eq!(ui.rows[ui.selected()].ctrl.name, "fov");
    }

    #[test]
    fn clicking_a_bar_sets_a_proportional_value() {
        let mut ui = laid_out();
        let hit = hit_of(&ui, "brightness");
        let (x0, x1) = hit.bar.expect("brightness draws a bar");

        // Far left is the minimum, far right the maximum.
        assert!(matches!(click_at(&mut ui, x0, hit.y), Action::Write { value, .. } if value == "0"));
        assert!(
            matches!(click_at(&mut ui, x1 - 1, hit.y), Action::Write { value, .. } if value == "255")
        );

        // The middle lands near the midpoint of 0..255.
        match click_at(&mut ui, x0 + (x1 - x0) / 2, hit.y) {
            Action::Write { value, .. } => {
                let v: i64 = value.parse().expect("number");
                assert!((120..=136).contains(&v), "midpoint click gave {v}");
            }
            _ => panic!("expected a write"),
        }
    }

    #[test]
    fn bar_clicks_snap_to_the_control_resolution() {
        let mut ui = laid_out();
        // White balance moves in steps of 10, but needs auto white balance off.
        ui.rows[2].value = Some("off".into());
        let hit = hit_of(&ui, "white_balance");
        let (x0, x1) = hit.bar.expect("bar");
        for x in x0..x1 {
            if let Action::Write { value, .. } = click_at(&mut ui, x, hit.y) {
                let v: i64 = value.parse().expect("number");
                assert_eq!(v % 10, 0, "{v} is not a multiple of the step");
                assert!((2000..=7500).contains(&v), "{v} out of range");
            }
        }
    }

    #[test]
    fn clicking_an_option_name_selects_that_option() {
        let mut ui = laid_out();
        let hit = hit_of(&ui, "fov");
        let (x0, _, name) = hit.choices.iter().find(|(_, _, n)| *n == "narrow").expect("narrow");
        assert!(matches!(click_at(&mut ui, *x0, hit.y), Action::Write { value, .. } if value == *name));
    }

    #[test]
    fn clicking_the_active_option_does_not_rewrite_it() {
        let mut ui = laid_out();
        let hit = hit_of(&ui, "fov"); // already "wide"
        let (x0, _, _) = hit.choices.iter().find(|(_, _, n)| *n == "wide").expect("wide");
        assert!(matches!(click_at(&mut ui, *x0, hit.y), Action::None));
    }

    #[test]
    fn clicking_a_locked_control_is_refused() {
        let mut ui = laid_out();
        let hit = hit_of(&ui, "white_balance"); // auto white balance is on
        let (x0, x1) = hit.bar.expect("bar");
        assert!(matches!(click_at(&mut ui, (x0 + x1) / 2, hit.y), Action::None));
        assert!(ui.status.contains("white_balance_auto is on"));
    }

    #[test]
    fn dragging_stays_on_the_control_it_started_on() {
        let mut ui = laid_out();
        let brightness = hit_of(&ui, "brightness");
        let contrast = hit_of(&ui, "contrast");
        ui.list.select(Some(brightness.index));

        let drag = |ui: &mut Ui, x: u16, y: u16| {
            ui.on_mouse(MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            })
        };
        let (x0, x1) = brightness.bar.expect("bar");
        assert!(matches!(drag(&mut ui, x1 - 1, brightness.y), Action::Write { .. }));
        // Straying onto another row must not start changing that one.
        assert!(matches!(drag(&mut ui, (x0 + x1) / 2, contrast.y), Action::None));
        assert_eq!(ui.selected(), brightness.index);
    }

    #[test]
    fn clicking_outside_any_control_does_nothing() {
        let mut ui = laid_out();
        // The header, above the list.
        assert!(matches!(click_at(&mut ui, 5, 1), Action::None));
    }

    #[test]
    fn scrolling_moves_the_selection() {
        let mut ui = laid_out();
        scroll(&mut ui, false);
        assert_eq!(ui.selected(), 1);
        scroll(&mut ui, true);
        assert_eq!(ui.selected(), 0);
    }

    #[test]
    fn a_click_dismisses_help() {
        let mut ui = laid_out();
        press(&mut ui, KeyCode::Char('?'));
        assert!(matches!(ui.mode, Mode::Help));
        click_at(&mut ui, 10, 10);
        assert!(matches!(ui.mode, Mode::Browse));
    }

    #[test]
    fn the_mouse_is_ignored_while_typing() {
        let mut ui = laid_out();
        press(&mut ui, KeyCode::Enter); // edit brightness
        let hit = hit_of(&ui, "fov");
        assert!(matches!(click_at(&mut ui, hit.choices[1].0, hit.y), Action::None));
        assert!(matches!(ui.mode, Mode::Edit { .. }));
    }

    #[test]
    fn bar_scales_within_range() {
        assert_eq!(bar(0, 0, 10, 4), "····");
        assert_eq!(bar(10, 0, 10, 4), "████");
        assert_eq!(bar(5, 0, 10, 4), "██··");
        // Degenerate input must not panic or divide by zero.
        assert_eq!(bar(5, 5, 5, 4), "");
        assert_eq!(bar(99, 0, 10, 4), "████");
    }
}
