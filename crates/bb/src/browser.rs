//! The real [`Browser`] implementation: shell out to the OS opener.

use crate::core::Browser;

/// Opens URLs with the platform's default handler (`open` / `xdg-open` / `start`).
pub struct SystemBrowser;

impl Browser for SystemBrowser {
    fn browse(&self, url: &str) -> Result<(), std::io::Error> {
        let (program, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
            ("open", vec![url])
        } else if cfg!(target_os = "windows") {
            ("cmd", vec!["/C", "start", "", url])
        } else {
            ("xdg-open", vec![url])
        };
        std::process::Command::new(program)
            .args(args)
            .status()
            .map(|_| ())
    }
}
