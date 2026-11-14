//! Showing only the game in a stream.
//!
//! Sunshine sends the whole screen. Without this the player sees the
//! desktop first: its wallpaper and icons, the store getting the game
//! ready, whatever was left open. So for the length of a stream the launcher
//! puts a plain CarlaOS screen over the desktop, a curtain:
//!
//! - before the game shows, it says the game is starting, and how far the
//!   store's update is when there is one;
//! - a game that fills the screen has the screen to itself: the curtain is
//!   hidden and nothing of CarlaOS is in its way;
//! - a game in a window, or its own launcher, is put in front of the
//!   curtain, so the player sees that window and nothing else;
//! - windows the store needs an answer in (a choice of how to start, a
//!   question about saves) are put in front of it as well;
//! - after the game it says goodbye until Sunshine ends the stream.
//!
//! It must never leave a player looking at a curtain with the thing they
//! need behind it. So it lifts by itself when no game has shown after a
//! while, and the Ally's quick menu can lift it at any time.
//!
//! This module decides ([`Stage`]); the Windows part at the end only looks
//! at the windows and does what was decided.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use carla_core::library::{Update, UpdateState};

use crate::files::Files;

/// With no game on the screen after this long, the curtain lifts: what is
/// behind it then says more than "starting".
pub const LIFT_AFTER: Duration = Duration::from_secs(75);

/// Programs whose windows may need an answer before a game can start.
const HELPERS: &[&str] = &[
    "steam.exe",
    "steamwebhelper.exe",
    "steamerrorreporter.exe",
    "epicgameslauncher.exe",
    "epicwebhelper.exe",
    "galaxyclient.exe",
    "eadesktop.exe",
    "ealauncher.exe",
    "upc.exe",
    "ubisoftconnect.exe",
    "socialclubhelper.exe",
    "battle.net.exe",
    "werfault.exe",
];

/// Their main windows, which are not questions.
const MAIN_WINDOWS: &[&str] = &["steam", "friends list", "epic games launcher", "gog galaxy", "ea", "ubisoft connect", "battle.net"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// A window of the game itself.
    Game,
    /// A window of the store that may need an answer.
    Helper,
}

/// One window on the desktop that matters to the curtain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub id: isize,
    pub role: Role,
    /// Fills the screen.
    pub covers: bool,
}

fn normal(path: &str) -> String {
    path.replace('\\', "/").to_lowercase()
}

/// What a window is to the curtain, from the program it belongs to. `None`
/// for everything that stays behind it.
pub fn role(exe: &str, title: &str, covers: bool, game_dir: &str) -> Option<Role> {
    let exe = normal(exe);
    let mut dir = normal(game_dir);
    if !dir.ends_with('/') {
        dir.push('/');
    }
    // A folder this short would make half the machine "the game".
    if dir.len() >= 4 && exe.starts_with(&dir) {
        return Some(Role::Game);
    }
    let name = exe.rsplit('/').next().unwrap_or_default();
    let title = title.trim().to_lowercase();
    let question = !title.is_empty() && !covers && !MAIN_WINDOWS.contains(&title.as_str());
    (HELPERS.contains(&name) && question).then_some(Role::Helper)
}

/// What the curtain says.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lines {
    pub title: String,
    pub note: String,
    /// Something is being waited for: the dots move.
    pub busy: bool,
}

/// What to do after one look at the desktop.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scene {
    /// Show the curtain with these words, or hide it.
    pub curtain: Option<Lines>,
    /// Windows that belong in front of the curtain, the one to end up on
    /// top last.
    pub raise: Vec<isize>,
    /// Store windows that were put in front earlier and no longer need to be.
    pub lower: Vec<isize>,
    /// The game's window, the first time it is seen: give it the keyboard
    /// and the controller.
    pub focus: Option<isize>,
}

/// One look at the desktop.
#[derive(Debug, Clone, Copy)]
pub struct Look<'a> {
    pub now: Instant,
    /// The windows that matter, front to back.
    pub windows: &'a [Window],
    /// The store is still updating the game.
    pub update: Option<Update>,
    /// The game has closed and the stream is about to end.
    pub closing: bool,
    /// The player asked to see the desktop.
    pub reveal: bool,
}

pub struct Stage {
    title: String,
    /// "Steam", for the note about an update.
    store: String,
    /// Since when nothing has explained the game not showing.
    waiting: Instant,
    seen: bool,
    gave_up: bool,
    revealed: bool,
    focused: bool,
    helpers: Vec<isize>,
}

fn update_note(store: &str, update: Update) -> String {
    let progress = match (update.percent(), update.left()) {
        (Some(percent), Some(left)) => format!(" · {percent}% · {left} to go"),
        _ => String::new(),
    };
    match update.state {
        UpdateState::Running => format!("{store} is updating it first{progress}"),
        UpdateState::Needed => format!("It needs an update first. {store} is getting it ready{progress}"),
        UpdateState::Paused => format!("Its update is paused on the desktop{progress}"),
    }
}

impl Stage {
    pub fn new(title: &str, store: &str, now: Instant) -> Self {
        Self { title: title.to_string(), store: store.to_string(), waiting: now, seen: false, gave_up: false, revealed: false, focused: false, helpers: Vec::new() }
    }

    /// Whether the curtain has been lifted for good or for now.
    pub fn lifted(&self) -> bool {
        self.revealed || self.gave_up
    }

    pub fn tick(&mut self, look: Look) -> Scene {
        let games: Vec<Window> = look.windows.iter().copied().filter(|w| w.role == Role::Game).collect();
        let helpers = look.windows.iter().filter(|w| w.role == Role::Helper).map(|w| w.id);
        self.revealed |= look.reveal;
        if !games.is_empty() {
            self.seen = true;
            // It came after all: the curtain is wanted again around it.
            self.gave_up = false;
        }
        // An update explains the wait, so the patience starts again after it.
        if look.update.is_some() || self.seen {
            self.waiting = look.now;
        } else if look.now.duration_since(self.waiting) > LIFT_AFTER {
            self.gave_up = true;
        }
        let mut scene = Scene::default();
        if !self.focused {
            scene.focus = games.iter().find(|w| w.covers).or(games.first()).map(|w| w.id);
            self.focused = scene.focus.is_some();
        }
        let filled = games.iter().any(|w| w.covers);
        if !self.lifted() && !filled {
            // Front to back was how they were seen; the front one is raised last.
            scene.raise = if games.is_empty() { helpers.rev().collect() } else { games.iter().rev().map(|w| w.id).collect() };
            scene.curtain = Some(self.lines(&games, look));
        }
        let wanted: Vec<isize> = if games.is_empty() { scene.raise.clone() } else { Vec::new() };
        scene.lower = self.helpers.iter().copied().filter(|id| !wanted.contains(id)).collect();
        self.helpers = wanted;
        scene
    }

    fn lines(&self, games: &[Window], look: Look) -> Lines {
        if look.closing {
            return Lines { title: "See you soon".to_string(), note: format!("{} has closed.", self.title), busy: false };
        }
        if !games.is_empty() {
            // Only a backdrop for the game's own window.
            return Lines::default();
        }
        if self.seen {
            return Lines { title: self.title.clone(), note: "One moment".to_string(), busy: true };
        }
        let note = look.update.map(|u| update_note(&self.store, u)).unwrap_or_else(|| "One moment".to_string());
        Lines { title: format!("Starting {}", self.title), note, busy: true }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Told {
    update: Option<Update>,
    closing: bool,
    reveal: bool,
    stop: bool,
}

/// The curtain as the launcher holds it.
pub struct Curtain {
    told: Arc<Mutex<Told>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Curtain {
    /// Put the curtain up for one game. Away from Windows there is none and
    /// everything here does nothing.
    pub fn open(title: &str, store: &str, game_dir: &Path, files: &Files) -> Self {
        let told = Arc::new(Mutex::new(Told::default()));
        let thread = os::start(Stage::new(title, store, Instant::now()), game_dir, told.clone(), files.clone());
        Self { told, thread }
    }

    /// No curtain at all: the administrator turned it off.
    pub fn none() -> Self {
        Self { told: Arc::default(), thread: None }
    }

    fn tell(&self, change: impl FnOnce(&mut Told)) {
        change(&mut self.told.lock().unwrap_or_else(|e| e.into_inner()));
    }

    /// What the store's update is doing now.
    pub fn update(&self, update: Option<Update>) {
        self.tell(|t| t.update = update);
    }

    /// The game has closed.
    pub fn closing(&self) {
        self.tell(|t| t.closing = true);
    }

    /// The player asked to see the desktop.
    pub fn reveal(&self) {
        self.tell(|t| t.reveal = true);
    }

    /// Take it down and wait until it is gone.
    pub fn close(mut self) {
        self.tell(|t| t.stop = true);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(not(windows))]
mod os {
    use super::*;

    pub fn start(_stage: Stage, _game_dir: &Path, _told: Arc<Mutex<Told>>, _files: Files) -> Option<std::thread::JoinHandle<()>> {
        None
    }
}

#[cfg(windows)]
mod os {
    use super::*;
    use std::collections::HashMap;
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{
        BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, DrawTextW, Ellipse, EndPaint, FillRect, InvalidateRect, SelectObject, SetBkMode, SetTextColor, DT_CENTER,
        DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, PAINTSTRUCT, TRANSPARENT,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EnumWindows, GetClientRect, GetForegroundWindow, GetMessageW, GetSystemMetrics, GetWindowRect,
        GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, PostQuitMessage, RegisterClassW, SetCursor, SetForegroundWindow, SetTimer, SetWindowPos,
        ShowWindow, TranslateMessage, HWND_NOTOPMOST, HWND_TOPMOST, MSG, SM_CXSCREEN, SM_CYSCREEN, SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SWP_NOZORDER, SWP_SHOWWINDOW, SW_HIDE, WM_DESTROY, WM_DISPLAYCHANGE, WM_ERASEBKGND, WM_PAINT, WM_SETCURSOR, WM_TIMER, WNDCLASSW, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
    };

    /// How often the desktop is looked at.
    const EVERY_MS: u32 = 300;

    // The dark CarlaOS colours, as Windows wants them: blue, green, red.
    const BACK: u32 = 0x0020_121e;
    const TEXT: u32 = 0x00ef_ecf8;
    const QUIET: u32 = 0x00b3_a6b9;
    const ACCENT: u32 = 0x00bb_a7f2;
    const ACCENT_SOFT: u32 = 0x0050_355a;

    struct State {
        stage: Stage,
        dir: String,
        told: Arc<Mutex<Told>>,
        files: Files,
        lines: Lines,
        shown: bool,
        /// Windows put in front since the curtain was last shown.
        raised: Vec<isize>,
        /// The program each process belongs to, asked once.
        exes: HashMap<u32, String>,
        ticks: u32,
        said_lifted: bool,
    }

    /// One curtain per launcher, and the window procedure needs to find it.
    static STATE: Mutex<Option<State>> = Mutex::new(None);

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn screen() -> (i32, i32) {
        // SAFETY: plain questions about the main screen.
        unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
    }

    /// The program a process was started from, in full.
    fn exe_of(pid: u32) -> String {
        // SAFETY: the handle is checked and closed; the buffer is as long as
        // the length handed over.
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return String::new();
            }
            let mut name = [0u16; 1024];
            let mut len = name.len() as u32;
            let found = QueryFullProcessImageNameW(process, 0, name.as_mut_ptr(), &mut len) != 0;
            CloseHandle(process);
            if found {
                String::from_utf16_lossy(&name[..len as usize])
            } else {
                String::new()
            }
        }
    }

    unsafe extern "system" fn collect(window: HWND, list: LPARAM) -> i32 {
        // SAFETY: `list` is the vector `windows` handed to EnumWindows, which
        // has returned before that vector is used again.
        let list = unsafe { &mut *(list as *mut Vec<HWND>) };
        list.push(window);
        1
    }

    /// The windows that matter to the curtain, front to back.
    fn windows(own: HWND, state: &mut State) -> Vec<Window> {
        let mut all: Vec<HWND> = Vec::new();
        // SAFETY: `collect` only pushes into `all`, which outlives the call.
        unsafe { EnumWindows(Some(collect), &mut all as *mut Vec<HWND> as LPARAM) };
        let (sw, sh) = screen();
        let mut found = Vec::new();
        for window in all {
            // SAFETY: questions about a window handle Windows just gave us;
            // a window that has gone meanwhile answers "no" to all of them.
            unsafe {
                if window == own || IsWindowVisible(window) == 0 || IsIconic(window) != 0 {
                    continue;
                }
                let mut rect: RECT = std::mem::zeroed();
                if GetWindowRect(window, &mut rect) == 0 {
                    continue;
                }
                let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
                // Programs keep tiny windows around that nobody ever sees.
                if w < 64 || h < 64 {
                    continue;
                }
                let mut pid = 0u32;
                GetWindowThreadProcessId(window, &mut pid);
                if pid == 0 || pid == std::process::id() {
                    continue;
                }
                let exe = state.exes.entry(pid).or_insert_with(|| exe_of(pid)).clone();
                if exe.is_empty() {
                    continue;
                }
                // How much of the main screen it covers.
                let across = (rect.right.min(sw) - rect.left.max(0)).max(0) as i64;
                let down = (rect.bottom.min(sh) - rect.top.max(0)).max(0) as i64;
                let covers = across * down * 100 >= i64::from(sw) * i64::from(sh) * 95;
                let mut title = [0u16; 256];
                let len = GetWindowTextW(window, title.as_mut_ptr(), title.len() as i32).max(0) as usize;
                let title = String::from_utf16_lossy(&title[..len]);
                if let Some(role) = role(&exe, &title, covers, &state.dir) {
                    found.push(Window { id: window as isize, role, covers });
                }
            }
        }
        // Process numbers are handed out again; forget those that are gone.
        if state.ticks % 100 == 0 {
            state.exes.clear();
        }
        found
    }

    fn place(window: isize, after: HWND) {
        // SAFETY: asks Windows to reorder a window, without waiting for a
        // program that does not answer. A window that is gone is ignored.
        unsafe { SetWindowPos(window as HWND, after, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS) };
    }

    /// Give a window the keyboard, which Windows only lets the program in
    /// front do: so join the one in front for a moment.
    fn bring_forward(window: isize) {
        // SAFETY: plain window calls; the attachment is undone at once.
        unsafe {
            let front = GetForegroundWindow();
            let me = GetCurrentThreadId();
            let other = if front.is_null() { 0 } else { GetWindowThreadProcessId(front, null_mut()) };
            if other != 0 && other != me {
                AttachThreadInput(me, other, 1);
                SetForegroundWindow(window as HWND);
                AttachThreadInput(me, other, 0);
            } else {
                SetForegroundWindow(window as HWND);
            }
        }
    }

    /// One look at the desktop, and doing what the stage decides.
    fn tick(own: HWND) {
        let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
        let Some(state) = guard.as_mut() else { return };
        let told = *state.told.lock().unwrap_or_else(|e| e.into_inner());
        if told.stop {
            let raised = std::mem::take(&mut state.stage.helpers);
            drop(guard);
            raised.into_iter().for_each(|w| place(w, HWND_NOTOPMOST));
            // SAFETY: our own window, on the thread that made it.
            unsafe { DestroyWindow(own) };
            return;
        }
        state.ticks = state.ticks.wrapping_add(1);
        let seen = windows(own, state);
        let scene = state.stage.tick(Look { now: Instant::now(), windows: &seen, update: told.update, closing: told.closing, reveal: told.reveal });
        if state.stage.lifted() && !state.said_lifted {
            state.said_lifted = true;
            state.files.log(if told.reveal { "The loading screen was taken away from the Ally." } else { "The game did not show: the loading screen was taken away." });
        }
        let was_shown = state.shown;
        let (mut repaint, mut dots) = (false, false);
        let mut raise = Vec::new();
        match &scene.curtain {
            Some(lines) => {
                if !was_shown {
                    state.raised.clear();
                }
                repaint = *lines != state.lines || !was_shown;
                dots = lines.busy && !repaint;
                state.lines = lines.clone();
                state.shown = true;
                raise = scene.raise.iter().copied().filter(|w| !state.raised.contains(w)).collect();
                state.raised.retain(|w| scene.raise.contains(w));
                state.raised.extend(&raise);
            }
            None => {
                state.shown = false;
                state.raised.clear();
            }
        }
        let games: Vec<isize> = seen.iter().filter(|w| w.role == Role::Game).map(|w| w.id).collect();
        let shown = state.shown;
        // Everything below talks to windows, ours and others', which can
        // call back into this program: not while the state is held.
        drop(guard);
        let (sw, sh) = screen();
        // SAFETY: calls on our own window from the thread that made it.
        unsafe {
            if shown && !was_shown {
                SetWindowPos(own, HWND_TOPMOST, 0, 0, sw, sh, SWP_NOACTIVATE | SWP_SHOWWINDOW);
            } else if !shown && was_shown {
                ShowWindow(own, SW_HIDE);
            }
            if shown && repaint {
                InvalidateRect(own, null(), 0);
            } else if shown && dots {
                // Only the moving dots, so the words do not flicker.
                let around = RECT { left: sw / 3, right: sw - sw / 3, top: sh * 63 / 100, bottom: sh * 73 / 100 };
                InvalidateRect(own, &around, 0);
            }
        }
        scene.lower.into_iter().for_each(|w| place(w, HWND_NOTOPMOST));
        raise.into_iter().for_each(|w| place(w, HWND_TOPMOST));
        if let Some(window) = scene.focus {
            // A game that already took the keyboard itself is left alone.
            // SAFETY: a plain question.
            let front = unsafe { GetForegroundWindow() } as isize;
            if !games.contains(&front) {
                bring_forward(window);
            }
        }
    }

    fn paint(own: HWND) {
        let (lines, ticks) = match STATE.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            Some(state) => (state.lines.clone(), state.ticks),
            None => (Lines::default(), 0),
        };
        // SAFETY: the usual painting calls on our own window; every object
        // made here is selected out and deleted before leaving.
        unsafe {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let dc = BeginPaint(own, &mut ps);
            let mut all: RECT = std::mem::zeroed();
            GetClientRect(own, &mut all);
            let back = CreateSolidBrush(BACK);
            FillRect(dc, &all, back);
            DeleteObject(back);
            SetBkMode(dc, TRANSPARENT as _);
            let (w, h) = (all.right, all.bottom);
            let face = wide("Segoe UI");
            let font = |size: i32, weight: i32| CreateFontW(-size, 0, 0, 0, weight, 0, 0, 0, 1, 0, 0, 5, 0, face.as_ptr());
            let text = |words: &str, size: i32, weight: i32, colour: u32, middle: i32| {
                if words.is_empty() {
                    return;
                }
                let f = font(size, weight);
                let before = SelectObject(dc, f);
                SetTextColor(dc, colour);
                let mut at = RECT { left: w / 12, right: w - w / 12, top: middle - size, bottom: middle + size };
                let words = wide(words);
                DrawTextW(dc, words.as_ptr(), -1, &mut at, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS);
                SelectObject(dc, before);
                DeleteObject(f);
            };
            if !lines.title.is_empty() {
                text("CarlaOS", h / 36, 600, ACCENT, h * 30 / 100);
            }
            text(&lines.title, h / 13, 700, TEXT, h * 45 / 100);
            text(&lines.note, h / 30, 400, QUIET, h * 57 / 100);
            if lines.busy {
                // Three dots, one of them lit, moving along.
                let (r, gap) = ((h / 110).max(3), (h / 28).max(10));
                for i in 0..3i32 {
                    let lit = (ticks / 2) % 3 == i as u32;
                    let brush = CreateSolidBrush(if lit { ACCENT } else { ACCENT_SOFT });
                    let before = SelectObject(dc, brush);
                    let pen = SelectObject(dc, windows_sys::Win32::Graphics::Gdi::GetStockObject(windows_sys::Win32::Graphics::Gdi::NULL_PEN));
                    let (x, y) = (w / 2 + (i - 1) * gap, h * 68 / 100);
                    Ellipse(dc, x - r, y - r, x + r, y + r);
                    SelectObject(dc, pen);
                    SelectObject(dc, before);
                    DeleteObject(brush);
                }
            }
            EndPaint(own, &ps);
        }
    }

    unsafe extern "system" fn proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match message {
            WM_TIMER => {
                tick(window);
                0
            }
            WM_PAINT => {
                paint(window);
                0
            }
            // Painted in one go, so nothing flickers.
            WM_ERASEBKGND => 1,
            // No mouse pointer on a loading screen.
            WM_SETCURSOR => {
                // SAFETY: no pointer is a valid pointer to set.
                unsafe { SetCursor(null_mut()) };
                1
            }
            // The screen was switched under us: cover the new size.
            WM_DISPLAYCHANGE => {
                let (w, h) = ((lparam & 0xffff) as i32, ((lparam >> 16) & 0xffff) as i32);
                // SAFETY: our own window.
                unsafe { SetWindowPos(window, null_mut(), 0, 0, w, h, SWP_NOZORDER | SWP_NOACTIVATE) };
                0
            }
            WM_DESTROY => {
                // SAFETY: ends this thread's message loop.
                unsafe { PostQuitMessage(0) };
                0
            }
            // SAFETY: everything else is Windows' own business.
            _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
        }
    }

    fn run() -> Result<(), &'static str> {
        // SAFETY: the usual making of a window and running of its messages,
        // all on this one thread.
        unsafe {
            let instance = GetModuleHandleW(null());
            let class = wide("CarlaOSCurtain");
            let mut wc: WNDCLASSW = std::mem::zeroed();
            wc.lpfnWndProc = Some(proc);
            wc.hInstance = instance;
            wc.lpszClassName = class.as_ptr();
            if RegisterClassW(&wc) == 0 {
                return Err("Windows did not accept the loading screen's window class.");
            }
            let (sw, sh) = screen();
            let name = wide("CarlaOS");
            // On top of everything, never taking the keyboard, not in the
            // taskbar. Made hidden: the first look decides whether to show it.
            let window =
                CreateWindowExW(WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW, class.as_ptr(), name.as_ptr(), WS_POPUP, 0, 0, sw, sh, null_mut(), null_mut(), instance, null());
            if window.is_null() {
                return Err("Windows did not make the loading screen's window.");
            }
            tick(window);
            SetTimer(window, 1, EVERY_MS, None);
            let mut message: MSG = std::mem::zeroed();
            while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        Ok(())
    }

    pub fn start(stage: Stage, game_dir: &Path, told: Arc<Mutex<Told>>, files: Files) -> Option<std::thread::JoinHandle<()>> {
        let state = State {
            stage,
            dir: game_dir.to_string_lossy().into_owned(),
            told,
            files: files.clone(),
            lines: Lines::default(),
            shown: false,
            raised: Vec::new(),
            exes: HashMap::new(),
            ticks: 0,
            said_lifted: false,
        };
        *STATE.lock().unwrap_or_else(|e| e.into_inner()) = Some(state);
        Some(std::thread::spawn(move || {
            if let Err(e) = run() {
                files.log(e);
            }
            *STATE.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIR: &str = r"D:\SteamLibrary\steamapps\common\Hades";

    fn game(id: isize, covers: bool) -> Window {
        Window { id, role: Role::Game, covers }
    }

    fn helper(id: isize) -> Window {
        Window { id, role: Role::Helper, covers: false }
    }

    fn look(now: Instant, windows: &[Window]) -> Look<'_> {
        Look { now, windows, update: None, closing: false, reveal: false }
    }

    #[test]
    fn a_window_is_the_game_when_its_program_is_in_the_games_folder() {
        assert_eq!(role(r"D:\SteamLibrary\steamapps\common\Hades\x64\Hades.exe", "Hades", true, DIR), Some(Role::Game));
        assert_eq!(role(r"d:/steamlibrary/steamapps/common/hades/Launcher.exe", "", false, DIR), Some(Role::Game), "however the path is written");
        assert_eq!(role(r"D:\SteamLibrary\steamapps\common\Hades II\Hades2.exe", "Hades II", true, DIR), None, "not a folder that only starts the same");
        assert_eq!(role(r"C:\Windows\explorer.exe", "Documents", false, DIR), None);
        assert_eq!(role(r"C:\Windows\explorer.exe", "Documents", false, "C:"), None, "a folder too short to mean one game");
    }

    #[test]
    fn a_stores_questions_come_in_front_and_its_main_window_does_not() {
        let steam = r"C:\Program Files (x86)\Steam\bin\cef\cef.win7x64\steamwebhelper.exe";
        assert_eq!(role(steam, "Hades - Steam", false, DIR), Some(Role::Helper));
        assert_eq!(role(steam, "Steam", false, DIR), None);
        assert_eq!(role(steam, "Friends List", false, DIR), None);
        assert_eq!(role(steam, "", false, DIR), None, "windows without a name are not questions");
        assert_eq!(role(steam, "Steam Big Picture Mode", true, DIR), None, "nothing of a store may fill the screen");
        assert_eq!(role(r"C:\Windows\System32\WerFault.exe", "Hades.exe", false, DIR), Some(Role::Helper), "a crash is worth seeing");
    }

    #[test]
    fn the_desktop_is_covered_until_the_game_fills_the_screen() {
        let t0 = Instant::now();
        let mut stage = Stage::new("Hades", "Steam", t0);
        let first = stage.tick(look(t0, &[]));
        assert_eq!(first.curtain, Some(Lines { title: "Starting Hades".into(), note: "One moment".into(), busy: true }));
        assert!(first.raise.is_empty() && first.focus.is_none());
        // The game shows and fills the screen: it has the screen to itself.
        let playing = stage.tick(look(t0 + Duration::from_secs(8), &[game(7, true)]));
        assert_eq!((playing.curtain, playing.raise, playing.focus), (None, vec![], Some(7)));
        // It gets the keyboard once, not at every look.
        assert_eq!(stage.tick(look(t0 + Duration::from_secs(9), &[game(7, true)])).focus, None);
        // The game has gone: the desktop stays covered until the stream ends.
        let gone = stage.tick(look(t0 + Duration::from_secs(900), &[]));
        assert_eq!(gone.curtain.map(|l| l.title).as_deref(), Some("Hades"));
        let closing = stage.tick(Look { closing: true, ..look(t0 + Duration::from_secs(902), &[]) });
        assert_eq!(closing.curtain, Some(Lines { title: "See you soon".into(), note: "Hades has closed.".into(), busy: false }));
    }

    #[test]
    fn a_game_in_a_window_is_shown_in_front_of_the_curtain() {
        let t0 = Instant::now();
        let mut stage = Stage::new("Hades", "Steam", t0);
        // Its own launcher first, with a dialog in front of it.
        let scene = stage.tick(look(t0, &[game(2, false), game(1, false), helper(9)]));
        assert_eq!(scene.curtain, Some(Lines::default()), "a plain backdrop");
        assert_eq!(scene.raise, [1, 2], "the front one is raised last, and the store stays behind");
        assert_eq!(scene.focus, Some(2));
        // The launcher hands over to the game proper.
        let scene = stage.tick(look(t0 + Duration::from_secs(5), &[game(3, true)]));
        assert_eq!(scene.curtain, None);
    }

    #[test]
    fn a_question_from_the_store_is_shown_and_put_back_after() {
        let t0 = Instant::now();
        let mut stage = Stage::new("Hades", "Steam", t0);
        let asked = stage.tick(look(t0, &[helper(9)]));
        assert_eq!((asked.raise, asked.lower), (vec![9], vec![]));
        assert!(asked.curtain.is_some());
        // Answered: the game comes, and the store's window is let go of.
        let after = stage.tick(look(t0 + Duration::from_secs(3), &[game(7, true), helper(9)]));
        assert_eq!((after.raise, after.lower), (vec![], vec![9]));
        assert!(stage.tick(look(t0 + Duration::from_secs(4), &[game(7, true)])).lower.is_empty(), "once");
    }

    #[test]
    fn an_update_is_shown_and_waited_for() {
        let t0 = Instant::now();
        let mut stage = Stage::new("Hades", "Steam", t0);
        let update = Update { state: UpdateState::Running, bytes: 2_000_000_000, done: 860_000_000 };
        let long = t0 + LIFT_AFTER * 10;
        let scene = stage.tick(Look { update: Some(update), ..look(long, &[]) });
        assert_eq!(scene.curtain.unwrap().note, "Steam is updating it first · 43% · 1.1 GB to go");
        // Done: the game gets its usual time to show from here.
        assert!(stage.tick(look(long + LIFT_AFTER, &[])).curtain.is_some());
        assert!(stage.tick(look(long + LIFT_AFTER + Duration::from_secs(1), &[])).curtain.is_none());
        let unknown = Update { state: UpdateState::Needed, bytes: 0, done: 0 };
        assert_eq!(update_note("Steam", unknown), "It needs an update first. Steam is getting it ready");
        assert_eq!(update_note("Steam", Update { state: UpdateState::Paused, ..update }), "Its update is paused on the desktop · 43% · 1.1 GB to go");
    }

    #[test]
    fn the_curtain_never_stays_in_front_of_what_the_player_needs() {
        let t0 = Instant::now();
        // No game after a while: lift, and let go of what was raised.
        let mut stage = Stage::new("Hades", "Steam", t0);
        assert_eq!(stage.tick(look(t0 + LIFT_AFTER, &[helper(9)])).raise, [9]);
        let lifted = stage.tick(look(t0 + LIFT_AFTER + Duration::from_secs(1), &[helper(9)]));
        assert_eq!((lifted.curtain, lifted.raise, lifted.lower), (None, vec![], vec![9]));
        assert!(stage.lifted());
        // The game comes after all, in a window: the backdrop is wanted again.
        assert!(stage.tick(look(t0 + LIFT_AFTER * 2, &[game(1, false)])).curtain.is_some());

        // The player asks to see the desktop: gone for the rest of the stream.
        let mut stage = Stage::new("Hades", "Steam", t0);
        assert!(stage.tick(Look { reveal: true, ..look(t0, &[]) }).curtain.is_none());
        assert!(stage.tick(look(t0 + Duration::from_secs(5), &[game(1, false)])).curtain.is_none());
        assert!(stage.tick(Look { closing: true, ..look(t0 + Duration::from_secs(9), &[]) }).curtain.is_none());
    }
}
