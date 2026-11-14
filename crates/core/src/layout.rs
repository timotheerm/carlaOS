//! Which screens are on.
//!
//! A stream can get a screen of its own: a virtual one, made by the Virtual
//! Display Driver (an open-source driver the administrator installs once),
//! at exactly the Ally's size. For the length of the stream that screen is
//! the only one switched on, so the game, the mouse and every window are on
//! it, and nothing of the desktop's own screens is in the picture.
//!
//! Nothing here is written to the Windows settings. Putting the layout back
//! means asking Windows for the layout it has saved for the screens that are
//! connected, which is the one the user chose.

/// One screen Windows knows of, on or off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    /// The name its maker gave it ("VDD by MTT", "ROG PG279Q").
    pub name: String,
    /// The path Windows knows it by.
    pub path: String,
    /// The maker and product codes from its EDID, as Windows reports them.
    pub maker: u16,
    pub product: u16,
    /// Shows part of the desktop now.
    pub active: bool,
}

/// The maker code of the Virtual Display Driver ("MTT") and its product
/// code, in both byte orders: the EDID stores them one way round and Windows
/// has been seen to report them either way.
const MAKERS: [u16; 2] = [0x3694, 0x9436];
const PRODUCTS: [u16; 2] = [0x1337, 0x3713];

/// Whether a screen is the Virtual Display Driver's.
pub fn is_virtual(screen: &Screen) -> bool {
    let path = screen.path.to_uppercase();
    (MAKERS.contains(&screen.maker) && PRODUCTS.contains(&screen.product)) || path.contains("#MTT1337#") || screen.name.trim().eq_ignore_ascii_case("VDD by MTT")
}

#[cfg(windows)]
mod os {
    use super::Screen;
    use windows_sys::Win32::Devices::Display::{
        DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig, SetDisplayConfig, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME, DISPLAYCONFIG_MODE_INFO,
        DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_TARGET_DEVICE_NAME, QDC_ALL_PATHS, QDC_VIRTUAL_MODE_AWARE, SDC_ALLOW_CHANGES, SDC_APPLY, SDC_USE_DATABASE_CURRENT,
        SDC_USE_SUPPLIED_DISPLAY_CONFIG, SDC_VIRTUAL_MODE_AWARE,
    };
    use windows_sys::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS};
    use windows_sys::Win32::Graphics::Gdi::DISPLAYCONFIG_PATH_ACTIVE;

    /// "No index" in the halves of the packed index fields.
    const NONE: u32 = 0xffff;

    fn query() -> Result<(Vec<DISPLAYCONFIG_PATH_INFO>, Vec<DISPLAYCONFIG_MODE_INFO>), String> {
        let flags = QDC_ALL_PATHS | QDC_VIRTUAL_MODE_AWARE;
        loop {
            let (mut paths, mut modes) = (0u32, 0u32);
            // SAFETY: two counts for Windows to fill in.
            let result = unsafe { GetDisplayConfigBufferSizes(flags, &mut paths, &mut modes) };
            if result != ERROR_SUCCESS {
                return Err(format!("Windows did not list the screens (answer {result})."));
            }
            let mut path_list = vec![DISPLAYCONFIG_PATH_INFO::default(); paths as usize];
            let mut mode_list = vec![DISPLAYCONFIG_MODE_INFO::default(); modes as usize];
            // SAFETY: the lists are as long as the counts handed over.
            let result = unsafe { QueryDisplayConfig(flags, &mut paths, path_list.as_mut_ptr(), &mut modes, mode_list.as_mut_ptr(), std::ptr::null_mut()) };
            // The screens changed between the two questions: ask again.
            if result == ERROR_INSUFFICIENT_BUFFER {
                continue;
            }
            if result != ERROR_SUCCESS {
                return Err(format!("Windows did not list the screens (answer {result})."));
            }
            path_list.truncate(paths as usize);
            mode_list.truncate(modes as usize);
            return Ok((path_list, mode_list));
        }
    }

    fn text(wide: &[u16]) -> String {
        String::from_utf16_lossy(&wide[..wide.iter().position(|c| *c == 0).unwrap_or(wide.len())])
    }

    fn describe(path: &DISPLAYCONFIG_PATH_INFO) -> Option<Screen> {
        let mut name = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
        name.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
        name.header.size = std::mem::size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
        name.header.adapterId = path.targetInfo.adapterId;
        name.header.id = path.targetInfo.id;
        // SAFETY: the header says how large the whole packet is.
        let result = unsafe { DisplayConfigGetDeviceInfo(&mut name.header) };
        (result == 0).then(|| Screen {
            name: text(&name.monitorFriendlyDeviceName),
            path: text(&name.monitorDevicePath),
            maker: name.edidManufactureId,
            product: name.edidProductCodeId,
            active: path.flags & DISPLAYCONFIG_PATH_ACTIVE != 0,
        })
    }

    /// Every connected screen, once each.
    pub fn list() -> Vec<Screen> {
        let Ok((paths, _)) = query() else { return Vec::new() };
        let mut found: Vec<Screen> = Vec::new();
        // Windows lists every way each screen could be driven; a screen
        // counts as on when any of them is.
        for path in paths.iter().filter(|p| p.targetInfo.targetAvailable != 0) {
            let Some(screen) = describe(path) else { continue };
            match found.iter_mut().find(|s| s.path == screen.path) {
                Some(known) => known.active |= screen.active,
                None => found.push(screen),
            }
        }
        found
    }

    /// Switch every screen off but the first one `wanted` picks, for now
    /// only.
    pub fn only(wanted: &dyn Fn(&Screen) -> bool) -> Result<(), String> {
        let (paths, _) = query()?;
        // The best way to drive it is the one Windows lists first, unless
        // one is already in use.
        let candidates: Vec<(DISPLAYCONFIG_PATH_INFO, Screen)> =
            paths.iter().filter(|p| p.targetInfo.targetAvailable != 0).filter_map(|p| describe(p).filter(|s| wanted(s)).map(|s| (*p, s))).collect();
        let mut path = candidates.iter().find(|(_, s)| s.active).or(candidates.first()).map(|(p, _)| *p).ok_or("The screen for the stream is not there.")?;
        // A path of its own, with no modes: Windows fills them in.
        path.flags = DISPLAYCONFIG_PATH_ACTIVE;
        // The packed index fields, as QDC_VIRTUAL_MODE_AWARE lays them out:
        // clone group in the low half, source mode in the high half; desktop
        // then target mode likewise. Writing a union field is safe.
        path.sourceInfo.Anonymous.Anonymous._bitfield = NONE << 16;
        path.targetInfo.Anonymous.Anonymous._bitfield = NONE << 16 | NONE;
        let flags = SDC_APPLY | SDC_USE_SUPPLIED_DISPLAY_CONFIG | SDC_ALLOW_CHANGES | SDC_VIRTUAL_MODE_AWARE;
        // SAFETY: one path and no modes, as counted.
        let result = unsafe { SetDisplayConfig(1, &path, 0, std::ptr::null(), flags) };
        if result == 0 {
            Ok(())
        } else {
            Err(format!("Windows did not switch to the stream's screen (answer {result})."))
        }
    }

    /// The layout Windows has saved for the screens that are connected.
    pub fn restore() -> Result<(), String> {
        // SAFETY: no paths and no modes, as the flag asks.
        let result = unsafe { SetDisplayConfig(0, std::ptr::null(), 0, std::ptr::null(), SDC_APPLY | SDC_USE_DATABASE_CURRENT) };
        if result == 0 {
            Ok(())
        } else {
            Err(format!("Windows did not put the screens back (answer {result})."))
        }
    }
}

#[cfg(not(windows))]
mod os {
    use super::Screen;

    pub fn list() -> Vec<Screen> {
        Vec::new()
    }

    pub fn only(_wanted: &dyn Fn(&Screen) -> bool) -> Result<(), String> {
        Err("Screens can only be switched on Windows.".to_string())
    }

    pub fn restore() -> Result<(), String> {
        Ok(())
    }
}

/// Every connected screen.
pub fn list() -> Vec<Screen> {
    os::list()
}

/// Make the first screen `wanted` picks the only one that is on, until
/// [`restore`].
pub fn only(wanted: &dyn Fn(&Screen) -> bool) -> Result<(), String> {
    os::only(wanted)
}

/// Put back the layout the user chose for the screens that are connected.
pub fn restore() -> Result<(), String> {
    os::restore()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(name: &str, path: &str, maker: u16, product: u16) -> Screen {
        Screen { name: name.into(), path: path.into(), maker, product, active: false }
    }

    #[test]
    fn the_virtual_display_drivers_screen_is_recognised() {
        let vdd = r"\\?\DISPLAY#MTT1337#1&1a2b3c4d&0&UID256#{e6f07b5f-ee97-4a90-b076-33f57bf4eaa7}";
        assert!(is_virtual(&screen("VDD by MTT", vdd, 0x3694, 0x1337)));
        assert!(is_virtual(&screen("", vdd, 0, 0)), "by its path alone");
        assert!(is_virtual(&screen("", "", 0x9436, 0x3713)), "codes the other way round");
        assert!(is_virtual(&screen("vdd by mtt ", "", 0, 0)));
        let monitor = r"\\?\DISPLAY#AUS27D1#5&4fd2de4&5&UID4352#{e6f07b5f-ee97-4a90-b076-33f57bf4eaa7}";
        assert!(!is_virtual(&screen("ROG PG279Q", monitor, 0x0469, 0x27d1)));
        assert!(!is_virtual(&screen("Generic PnP Monitor", "", 0x3694, 0x0001)), "the maker alone is not enough");
    }
}
