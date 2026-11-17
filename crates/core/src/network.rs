//! What the Ally is connected to, for the side menu and the top bar.
//!
//! Windows is asked through its own `netsh wlan show interfaces`. Its answer
//! is written in the language Windows is set to, so only the parts that are
//! the same in every language are read: the line named `SSID`, and the one
//! value that is a percentage.

use std::process::Command;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Wifi {
    pub name: String,
    /// 0 to 100.
    pub signal: u8,
}

/// Read Windows' answer. Nothing when no Wi-Fi network is connected.
pub fn parse(answer: &str) -> Option<Wifi> {
    let mut name = None;
    let mut signal = None;
    for line in answer.lines() {
        let Some((key, value)) = line.split_once(':') else { continue };
        let (key, value) = (key.trim(), value.trim());
        // "BSSID" and "AP BSSID" are the access point's address, not the name.
        if key == "SSID" && name.is_none() && !value.is_empty() {
            name = Some(value.to_string());
        } else if signal.is_none() {
            if let Some(percent) = value.strip_suffix('%').and_then(|n| n.trim().parse::<u8>().ok()) {
                signal = Some(percent.min(100));
            }
        }
    }
    Some(Wifi { name: name?, signal: signal.unwrap_or(0) })
}

#[cfg(windows)]
fn hidden(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hidden(_: &mut Command) {}

/// The Wi-Fi network this machine is on right now, if any. Away from Windows
/// nothing is known.
pub fn wifi() -> Option<Wifi> {
    if !cfg!(windows) {
        return None;
    }
    let mut command = Command::new("netsh.exe");
    command.args(["wlan", "show", "interfaces"]);
    hidden(&mut command);
    let output = command.output().ok()?;
    parse(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENGLISH: &str = "
There is 1 interface on the system:

    Name                   : Wi-Fi
    Description            : MediaTek Wi-Fi 6E MT7922 160MHz Wireless LAN Card
    GUID                   : 11111111-2222-3333-4444-555555555555
    Physical address       : aa:bb:cc:dd:ee:ff
    State                  : connected
    SSID                   : Maison Rose
    BSSID                  : 11:22:33:44:55:66
    Network type           : Infrastructure
    Radio type             : 802.11ax
    Channel                : 44
    Receive rate (Mbps)    : 1201
    Transmit rate (Mbps)   : 1201
    Signal                 : 86%
    Profile                : Maison Rose
";

    const FRENCH: &str = "
Il existe 1 interface sur le système :

    Nom                    : Wi-Fi
    État                   : connecté
    SSID                   : Livebox: 5 GHz
    BSSID                  : 11:22:33:44:55:66
    Signal                 : 64 %
";

    const OFF: &str = "
There is 1 interface on the system:

    Name                   : Wi-Fi
    State                  : disconnected
    Radio status           : Hardware On
                             Software On
";

    #[test]
    fn reads_the_network_and_its_strength() {
        assert_eq!(parse(ENGLISH), Some(Wifi { name: "Maison Rose".into(), signal: 86 }));
    }

    #[test]
    fn reads_windows_in_another_language_and_names_with_a_colon() {
        assert_eq!(parse(FRENCH), Some(Wifi { name: "Livebox: 5 GHz".into(), signal: 64 }));
    }

    #[test]
    fn no_network_is_nothing() {
        assert_eq!(parse(OFF), None);
        assert_eq!(parse(""), None);
    }
}
