//! CarlaOS desktop companion.
//!
//! M0 only proves the build and packaging path: it prints its self-description.
//! The service, API and Sunshine integration arrive in M3.

use carla_core::{AppInfo, Role};

fn main() {
    let info = AppInfo::current(Role::Companion);
    println!(
        "{}",
        serde_json::to_string_pretty(&info).expect("AppInfo is always serialisable")
    );
}
