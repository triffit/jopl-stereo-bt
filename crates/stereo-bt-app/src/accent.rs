//! The user's Windows accent colour, so the app matches Windows Settings.

/// An opaque RGB colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rgb {
    pub(crate) r: u8,
    pub(crate) g: u8,
    pub(crate) b: u8,
}

/// The accent colour Windows uses for controls in dark mode (the second
/// lighter shade of the user's accent, as in Settings), if available.
#[cfg(windows)]
pub(crate) fn system_accent() -> Option<Rgb> {
    use windows::UI::ViewManagement::{UIColorType, UISettings};

    let color = UISettings::new()
        .ok()?
        .GetColorValue(UIColorType::AccentLight2)
        .ok()?;
    Some(Rgb {
        r: color.R,
        g: color.G,
        b: color.B,
    })
}

/// Other platforms keep the app's built-in accent.
#[cfg(not(windows))]
pub(crate) fn system_accent() -> Option<Rgb> {
    None
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    #[ignore = "reads the accent colour of the signed-in Windows user"]
    fn reads_the_windows_accent() {
        let accent = system_accent().unwrap();
        println!("accent {accent:?}");
    }
}
