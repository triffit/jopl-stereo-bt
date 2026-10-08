//! Compiles the Slint UI (Fluent dark widget style) and, for Windows targets,
//! embeds the app icon and version information in the executable.

// The icon drawing and `.ico` encoder are shared with the app (tray icon, tests).
#[path = "src/ico.rs"]
mod ico;
#[path = "src/icon.rs"]
mod icon;

use std::error::Error;
use std::path::PathBuf;

/// Icon sizes Windows picks from for taskbar, Explorer and Alt+Tab.
const ICON_SIZES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 256];

fn main() -> Result<(), Box<dyn Error>> {
    let config = slint_build::CompilerConfiguration::new().with_style("fluent-dark".into());
    slint_build::compile_with_config("ui/app.slint", config)?;

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_windows_resources()?;
    }
    println!("cargo:rerun-if-changed=src/icon.rs");
    println!("cargo:rerun-if-changed=src/ico.rs");
    Ok(())
}

fn embed_windows_resources() -> Result<(), Box<dyn Error>> {
    let images: Vec<(u32, Vec<u8>)> = ICON_SIZES
        .iter()
        .map(|&size| (size, icon::rgba(size)))
        .collect();
    let icon_path = PathBuf::from(std::env::var("OUT_DIR")?).join("app.ico");
    std::fs::write(&icon_path, ico::encode(&images))?;

    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon(icon_path.to_str().ok_or("OUT_DIR is not valid UTF-8")?)
        .set("ProductName", "JOPL-StereoBT")
        .set("FileDescription", "JOPL-StereoBT — every device, in sync")
        .set("CompanyName", "JOPL")
        .set("LegalCopyright", "© 2026 JOPL, Triffit · MIT licence");
    resource.compile()?;
    Ok(())
}
