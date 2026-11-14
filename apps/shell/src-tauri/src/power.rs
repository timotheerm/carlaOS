//! Each game's power level and screen speed, put into effect. See
//! `carla_core::power` for what they mean and how they are chosen.
//!
//! On the Ally the power level goes to ASUS's own power mode (the one
//! Armoury Crate's Silent, Performance and Turbo set) and to the Windows
//! power mode. Away from an ASUS machine only the Windows power mode
//! changes. The screen speed is changed for the moment only, never saved:
//! Windows goes back to its own setting when CarlaOS puts it back, and at
//! the latest when the Ally restarts.

use std::sync::atomic::{AtomicBool, Ordering};

use carla_core::power::{self, Kind, Power, Profile, Screen, RESTING};
use serde::Serialize;
use tauri::State;

use crate::library::{locked, Library};

/// Set when CarlaOS changed the screen's speed and has not put it back.
static SCREEN_CHANGED: AtomicBool = AtomicBool::new(false);

/// Put a game's profile into effect, as it starts or when it is changed
/// while it runs.
pub(crate) fn apply(profile: Profile, kind: Kind) {
    os::set_power(profile.power);
    if kind.sets_screen() {
        match os::rates().and_then(|offered| profile.screen.rate(&offered)) {
            Some(rate) if os::set_rate(rate) => SCREEN_CHANGED.store(true, Ordering::SeqCst),
            _ => {}
        }
    }
}

fn screen_back() {
    if SCREEN_CHANGED.swap(false, Ordering::SeqCst) {
        os::rate_back();
    }
}

/// Between games: the menus need almost nothing.
pub(crate) fn rest() {
    os::set_power(RESTING);
    screen_back();
}

/// CarlaOS is closing: Windows as most people leave it.
pub(crate) fn leave() {
    os::set_power(Power::Balanced);
    screen_back();
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerView {
    power: Power,
    screen: Screen,
    /// Chosen for this game, rather than what every game gets.
    chosen: bool,
    /// Whether the screen speed applies here: a stream picks its own.
    sets_screen: bool,
}

pub(crate) fn view(library: &Library, game_id: &str, place: &str) -> Result<PowerView, String> {
    let kind = Kind::of(place);
    let store = library.store();
    let profile = power::profile(&store, game_id, kind)?;
    Ok(PowerView { power: profile.power, screen: profile.screen, chosen: power::chosen(&store, game_id, kind)?.is_some(), sets_screen: kind.sets_screen() })
}

#[tauri::command(async)]
pub fn power_get(library: State<'_, Library>, game_id: String, place: String) -> Result<PowerView, String> {
    view(&library, &game_id, &place)
}

/// Choose a game's power level and screen speed. Anyone may: it is the
/// player's to feel. Takes effect at once when that game is running.
#[tauri::command(async)]
pub fn power_set(library: State<'_, Library>, game_id: String, place: String, power: Power, screen: Screen) -> Result<PowerView, String> {
    set(&library, &game_id, &place, Profile { power, screen })
}

pub(crate) fn set(library: &Library, game_id: &str, place: &str, profile: Profile) -> Result<PowerView, String> {
    let kind = Kind::of(place);
    power::choose(&mut library.store(), game_id, kind, Some(profile))?;
    let running = locked(&library.0.active).as_ref().is_some_and(|a| a.view.game_id == game_id && a.place == place);
    if running {
        apply(profile, kind);
    }
    view(library, game_id, place)
}

#[cfg(windows)]
mod os {
    use carla_core::power::{asus_request, Power, ASUS_CONTROL};
    use windows::core::{s, w, GUID};
    use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE};
    use windows::Win32::Graphics::Gdi::{
        ChangeDisplaySettingsExW, EnumDisplaySettingsW, CDS_TYPE, DEVMODEW, DISP_CHANGE_SUCCESSFUL, DM_DISPLAYFREQUENCY, ENUM_CURRENT_SETTINGS, ENUM_DISPLAY_SETTINGS_MODE,
    };
    use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
    use windows::Win32::System::IO::DeviceIoControl;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    /// ASUS's power mode, through the ACPI device Armoury Crate uses. Does
    /// nothing on a machine without it.
    fn set_asus(power: Power) {
        // SAFETY: the name is a constant; the handle is checked and closed.
        let Ok(device) = (unsafe {
            CreateFileW(w!("\\\\.\\ATKACPI"), (GENERIC_READ | GENERIC_WRITE).0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, None)
        }) else {
            return;
        };
        let request = asus_request(power);
        let mut answer = [0u8; 16];
        let mut returned = 0u32;
        // SAFETY: both buffers live until the call returns and their sizes
        // are the ones given.
        unsafe {
            let _ = DeviceIoControl(
                device,
                ASUS_CONTROL,
                Some(request.as_ptr().cast()),
                request.len() as u32,
                Some(answer.as_mut_ptr().cast()),
                answer.len() as u32,
                Some(&mut returned),
                None,
            );
            let _ = CloseHandle(device);
        }
    }

    /// The Windows power mode (the slider under Settings, Power). Windows
    /// has no published header for it, so it is looked up by name and
    /// skipped where it is missing.
    fn set_windows(power: Power) {
        type SetOverlay = unsafe extern "system" fn(*const GUID) -> u32;
        // SAFETY: powrprof.dll is part of Windows; the function, when there,
        // takes one GUID by pointer and returns an error code.
        unsafe {
            let Ok(library) = LoadLibraryW(w!("powrprof.dll")) else { return };
            let Some(found) = GetProcAddress(library, s!("PowerSetActiveOverlayScheme")) else { return };
            let set: SetOverlay = std::mem::transmute(found);
            let guid = GUID::from_u128(power.windows_overlay());
            let _ = set(&guid);
        }
    }

    pub fn set_power(power: Power) {
        set_asus(power);
        set_windows(power);
    }

    fn current() -> Option<DEVMODEW> {
        let mut mode = DEVMODEW { dmSize: std::mem::size_of::<DEVMODEW>() as u16, ..Default::default() };
        // SAFETY: `mode` is a valid place to write to, with its size set.
        unsafe { EnumDisplaySettingsW(None, ENUM_CURRENT_SETTINGS, &mut mode) }.as_bool().then_some(mode)
    }

    /// The refresh rates the main screen offers at its current size.
    pub fn rates() -> Option<Vec<u32>> {
        let now = current()?;
        let mut rates = Vec::new();
        for index in 0.. {
            let mut mode = DEVMODEW { dmSize: std::mem::size_of::<DEVMODEW>() as u16, ..Default::default() };
            // SAFETY: as above.
            if !unsafe { EnumDisplaySettingsW(None, ENUM_DISPLAY_SETTINGS_MODE(index), &mut mode) }.as_bool() {
                break;
            }
            if mode.dmPelsWidth == now.dmPelsWidth && mode.dmPelsHeight == now.dmPelsHeight && !rates.contains(&mode.dmDisplayFrequency) {
                rates.push(mode.dmDisplayFrequency);
            }
        }
        Some(rates)
    }

    /// Change the refresh rate for now, without saving it. True when it
    /// changed.
    pub fn set_rate(rate: u32) -> bool {
        let Some(mut mode) = current() else { return false };
        if mode.dmDisplayFrequency == rate {
            return false;
        }
        mode.dmDisplayFrequency = rate;
        mode.dmFields = DM_DISPLAYFREQUENCY;
        // SAFETY: `mode` is a complete description; flag 0 changes it for
        // now without writing it to the registry.
        unsafe { ChangeDisplaySettingsExW(None, Some(&mode), None, CDS_TYPE(0), None) == DISP_CHANGE_SUCCESSFUL }
    }

    /// Back to the screen setting saved in Windows.
    pub fn rate_back() {
        // SAFETY: no mode given means "the one in the registry".
        let _ = unsafe { ChangeDisplaySettingsExW(None, None, None, CDS_TYPE(0), None) };
    }
}

#[cfg(not(windows))]
mod os {
    use carla_core::power::Power;

    pub fn set_power(_: Power) {}
    pub fn rates() -> Option<Vec<u32>> {
        None
    }
    pub fn set_rate(_: u32) -> bool {
        false
    }
    pub fn rate_back() {}
}
