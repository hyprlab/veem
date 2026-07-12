//! Host platform detection (distro + desktop), used to tailor the keyring /
//! Secret Service setup guidance we show the user.
//!
//! This works both for a native build and inside the Flatpak sandbox: the
//! sandbox's own `/etc/os-release` describes the *runtime*, not the host, so we
//! prefer the host copy Flatpak mounts at `/run/host/os-release`. The desktop
//! session is read from the standard XDG env vars, which Flatpak passes through.

use std::fs;

/// Whether Veem is running inside a Flatpak sandbox.
pub fn is_flatpak() -> bool {
    std::path::Path::new("/.flatpak-info").exists()
}

/// Contents of the *host* os-release (falls back to the local one natively).
fn os_release() -> String {
    for path in ["/run/host/os-release", "/etc/os-release", "/usr/lib/os-release"] {
        if let Ok(text) = fs::read_to_string(path) {
            if !text.trim().is_empty() {
                return text;
            }
        }
    }
    String::new()
}

/// Value of a `KEY=value` field in os-release text, unquoted and lowercased.
fn field_in(text: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    text.lines().find_map(|line| {
        line.strip_prefix(&prefix)
            .map(|v| v.trim().trim_matches('"').to_ascii_lowercase())
    })
}

/// Whether the given os-release text describes Linux Mint.
fn text_is_mint(text: &str) -> bool {
    field_in(text, "ID").as_deref() == Some("linuxmint")
        || field_in(text, "ID_LIKE")
            .is_some_and(|v| v.split_whitespace().any(|id| id == "linuxmint"))
}

/// True on Linux Mint (matches `ID=linuxmint`, or Mint listed in `ID_LIKE`).
pub fn is_linux_mint() -> bool {
    text_is_mint(&os_release())
}

/// True when the current desktop session is Cinnamon (Mint's default).
pub fn is_cinnamon() -> bool {
    let has_cinnamon = |var: &str| {
        std::env::var(var)
            .map(|v| v.to_ascii_lowercase().contains("cinnamon"))
            .unwrap_or(false)
    };
    has_cinnamon("XDG_CURRENT_DESKTOP")
        || has_cinnamon("XDG_SESSION_DESKTOP")
        || has_cinnamon("DESKTOP_SESSION")
}

/// The combination we specifically tailor keyring guidance for.
pub fn is_mint_cinnamon() -> bool {
    is_linux_mint() && is_cinnamon()
}

#[cfg(test)]
mod tests {
    use super::{field_in, text_is_mint};

    const MINT: &str = r#"NAME="Linux Mint"
VERSION="22.3 (Zara)"
ID=linuxmint
ID_LIKE="ubuntu debian"
PRETTY_NAME="Linux Mint 22.3""#;

    const FEDORA: &str = "NAME=Fedora Linux\nID=fedora\nPRETTY_NAME=\"Fedora Linux 44\"";

    #[test]
    fn detects_mint_by_id() {
        assert!(text_is_mint(MINT));
        assert!(!text_is_mint(FEDORA));
        assert!(!text_is_mint(""));
    }

    #[test]
    fn detects_mint_when_only_in_id_like() {
        // A derivative that identifies as something else but lists Mint in ID_LIKE.
        let text = "ID=lmde\nID_LIKE=\"linuxmint debian\"";
        assert!(text_is_mint(text));
    }

    #[test]
    fn field_is_unquoted_and_lowercased() {
        assert_eq!(field_in(MINT, "ID").as_deref(), Some("linuxmint"));
        assert_eq!(field_in(MINT, "ID_LIKE").as_deref(), Some("ubuntu debian"));
        assert_eq!(field_in(MINT, "MISSING"), None);
    }
}
