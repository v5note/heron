//! Platform identity, resolved at compile time by the single allowed cfg site.

/// The three platforms Heron targets. `Linux` is not a v1 target but the enum keeps
/// architecture honest (design §3.1) and avoids an uninhabited variant later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlatformKind {
    Macos,
    Windows,
    Linux,
}

impl PlatformKind {
    /// Compile-time platform. Used only to pick defaults such as the projection mode.
    pub const fn current() -> Self {
        #[cfg(target_os = "macos")]
        {
            PlatformKind::Macos
        }
        #[cfg(target_os = "windows")]
        {
            PlatformKind::Windows
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            PlatformKind::Linux
        }
    }

    pub const fn is_windows(self) -> bool {
        matches!(self, PlatformKind::Windows)
    }
}

#[cfg(test)]
mod tests {
    use super::PlatformKind;

    #[test]
    fn current_platform_is_detected() {
        let p = PlatformKind::current();
        #[cfg(target_os = "macos")]
        assert_eq!(p, PlatformKind::Macos);
        #[cfg(target_os = "windows")]
        assert_eq!(p, PlatformKind::Windows);
        #[cfg(all(unix, not(target_os = "macos")))]
        assert_eq!(p, PlatformKind::Linux);
    }
}
