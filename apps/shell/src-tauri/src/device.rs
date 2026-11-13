//! The Ally's own controls, for the quick menu: sound, screen brightness,
//! battery, the controller while a game has it, and which window is in
//! front. Everything here asks Windows; away from Windows nothing is known
//! and nothing can be set.

use carla_core::quick::Pad;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Battery {
    pub percent: u8,
    pub charging: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Sound {
    /// 0 to 100.
    pub level: u8,
    pub muted: bool,
}

#[cfg(windows)]
mod os {
    use super::{Battery, Pad, Sound};
    use windows::core::{w, Result, BSTR, PCWSTR};
    use windows::Win32::System::Variant::VARIANT;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoSetProxyBlanket, CLSCTX_ALL, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL,
        RPC_C_IMP_LEVEL_IMPERSONATE,
    };
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::System::Wmi::{IWbemClassObject, IWbemLocator, IWbemServices, WbemLocator, WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_INFINITE};
    use windows::Win32::UI::Input::XboxController::{XInputGetState, XINPUT_STATE};
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow};

    /// Windows wants this once on every thread that talks to sound or WMI.
    /// Asking again on the same thread is harmless.
    fn com() {
        // SAFETY: plain initialisation; the result only says whether it was
        // already done.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    }

    pub fn battery() -> Option<Battery> {
        let mut status = SYSTEM_POWER_STATUS::default();
        // SAFETY: `status` is a valid place for Windows to write to.
        unsafe { GetSystemPowerStatus(&mut status) }.ok()?;
        // 128 means there is no battery; 255 that the level is not known.
        if status.BatteryFlag & 128 != 0 || status.BatteryLifePercent > 100 {
            return None;
        }
        Some(Battery { percent: status.BatteryLifePercent, charging: status.ACLineStatus == 1 })
    }

    /// The volume control of whatever Windows plays sound through right now.
    fn speakers() -> Result<IAudioEndpointVolume> {
        com();
        // SAFETY: standard COM calls with valid arguments.
        unsafe {
            let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            devices.GetDefaultAudioEndpoint(eRender, eMultimedia)?.Activate(CLSCTX_ALL, None)
        }
    }

    pub fn sound() -> Option<Sound> {
        let speakers = speakers().ok()?;
        // SAFETY: `speakers` is a live COM object.
        let (level, muted) = unsafe { (speakers.GetMasterVolumeLevelScalar().ok()?, speakers.GetMute().ok()?.as_bool()) };
        Some(Sound { level: (level * 100.0).round().clamp(0.0, 100.0) as u8, muted })
    }

    pub fn set_sound(sound: Sound) -> std::result::Result<(), String> {
        let set = || -> Result<()> {
            let speakers = speakers()?;
            // SAFETY: `speakers` is a live COM object; a null context is allowed.
            unsafe {
                speakers.SetMasterVolumeLevelScalar(f32::from(sound.level.min(100)) / 100.0, std::ptr::null())?;
                speakers.SetMute(sound.muted, std::ptr::null())
            }
        };
        set().map_err(|e| format!("Windows did not change the volume: {e}"))
    }

    /// Windows' own door to the built-in screen's backlight.
    fn screens() -> Result<IWbemServices> {
        com();
        // SAFETY: standard COM calls with valid arguments.
        unsafe {
            let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER)?;
            let services = locator.ConnectServer(&BSTR::from("ROOT\\WMI"), &BSTR::new(), &BSTR::new(), &BSTR::new(), 0, &BSTR::new(), None)?;
            CoSetProxyBlanket(&services, RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE, PCWSTR::null(), RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE, None, EOAC_NONE)?;
            Ok(services)
        }
    }

    /// The first object of a class: the built-in screen.
    fn first(services: &IWbemServices, class: &str) -> Option<IWbemClassObject> {
        // SAFETY: `services` is live; `row` and `got` are valid to write to.
        unsafe {
            let rows = services.ExecQuery(&BSTR::from("WQL"), &BSTR::from(format!("SELECT * FROM {class}")), WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY, None).ok()?;
            let mut row = [None];
            let mut got = 0;
            let _ = rows.Next(WBEM_INFINITE, &mut row, &mut got);
            row[0].take()
        }
    }

    fn property(object: &IWbemClassObject, name: PCWSTR) -> Option<VARIANT> {
        let mut value = VARIANT::default();
        // SAFETY: `object` is live and `value` is valid to write to.
        unsafe { object.Get(name, 0, &mut value, None, None) }.ok()?;
        Some(value)
    }

    pub fn brightness() -> Option<u8> {
        let screen = first(&screens().ok()?, "WmiMonitorBrightness")?;
        let level = i32::try_from(&property(&screen, w!("CurrentBrightness"))?).ok()?;
        u8::try_from(level.clamp(0, 100)).ok()
    }

    pub fn set_brightness(level: u8) -> std::result::Result<(), String> {
        let set = || -> Option<Result<()>> {
            let services = screens().ok()?;
            let screen = first(&services, "WmiMonitorBrightnessMethods")?;
            let path = BSTR::try_from(&property(&screen, w!("__PATH"))?).ok()?;
            // SAFETY: all objects are live and the out-pointers valid.
            Some(unsafe {
                (|| {
                    let mut class = None;
                    services.GetObject(&BSTR::from("WmiMonitorBrightnessMethods"), Default::default(), None, Some(&mut class), None)?;
                    let class = class.ok_or_else(windows::core::Error::empty)?;
                    let mut wanted = None;
                    class.GetMethod(w!("WmiSetBrightness"), 0, &mut wanted, std::ptr::null_mut())?;
                    let wanted = wanted.ok_or_else(windows::core::Error::empty)?.SpawnInstance(0)?;
                    wanted.Put(w!("Timeout"), 0, &VARIANT::from(0i32), 0)?;
                    wanted.Put(w!("Brightness"), 0, &VARIANT::from(level.min(100)), 0)?;
                    services.ExecMethod(&path, &BSTR::from("WmiSetBrightness"), Default::default(), None, &wanted, None, None)
                })()
            })
        };
        match set() {
            Some(Ok(())) => Ok(()),
            Some(Err(e)) => Err(format!("Windows did not change the brightness: {e}")),
            None => Err("This screen's brightness cannot be changed from here.".to_string()),
        }
    }

    /// Every connected controller as one, or nothing when none is connected.
    /// Works while another program is in front, which is the point.
    pub fn pad() -> Option<Pad> {
        let pads: Vec<Pad> = (0..4)
            .filter_map(|index| {
                let mut state = XINPUT_STATE::default();
                // SAFETY: `state` is a valid place for Windows to write to.
                (unsafe { XInputGetState(index, &mut state) } == 0).then(|| Pad { buttons: state.Gamepad.wButtons.0, stick: (state.Gamepad.sThumbLX, state.Gamepad.sThumbLY) })
            })
            .collect();
        (!pads.is_empty()).then(|| Pad::merged(pads))
    }

    /// The window in front right now, to give the front back to later.
    pub fn front() -> isize {
        // SAFETY: no arguments.
        unsafe { GetForegroundWindow() }.0 as isize
    }

    /// Put a window in front even though another program has the front.
    /// Windows only lets the program in front hand it over, so this briefly
    /// joins that program's input queue.
    pub fn bring_forward(window: isize) {
        // SAFETY: handles are only passed back to Windows, which checks them.
        unsafe {
            let ours = GetCurrentThreadId();
            let theirs = GetWindowThreadProcessId(GetForegroundWindow(), None);
            let joined = theirs != 0 && theirs != ours && AttachThreadInput(ours, theirs, true).as_bool();
            let _ = SetForegroundWindow(HWND(window as *mut _));
            if joined {
                let _ = AttachThreadInput(ours, theirs, false);
            }
        }
    }
}

#[cfg(not(windows))]
mod os {
    use super::{Battery, Pad, Sound};

    pub fn battery() -> Option<Battery> {
        None
    }
    pub fn sound() -> Option<Sound> {
        None
    }
    pub fn set_sound(_: Sound) -> Result<(), String> {
        Err("Sound is only controlled on the Ally.".to_string())
    }
    pub fn brightness() -> Option<u8> {
        None
    }
    pub fn set_brightness(_: u8) -> Result<(), String> {
        Err("Brightness is only controlled on the Ally.".to_string())
    }
    pub fn pad() -> Option<Pad> {
        None
    }
    pub fn front() -> isize {
        0
    }
    pub fn bring_forward(_: isize) {}
}

pub use os::*;
