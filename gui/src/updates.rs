//! Manual metadata check against this fork's release page, through bundled Python.
use std::path::Path;
use std::process::Command;

use serde::Deserialize;

use crate::toolchain::{self, Python};

#[derive(Clone, Debug, Deserialize)]
pub struct Info {
    pub name: String,
    pub current: bool,
    pub url: String,
}

pub fn check(python: &Python, home: &Path) -> Result<Info, String> {
    let mut command = Command::new(&python.program);
    command
        .args(&python.args)
        .args(["-I", "-X", "utf8"])
        .arg(home.join("update_check.py"))
        .args(["--current", env!("SWR_BUILD")]);
    toolchain::hide_console(&mut command);
    let output = command.output().map_err(|_| "Could not start the update check.".to_string())?;
    if !output.status.success() || output.stdout.len() > 1024 * 1024 {
        return Err("Could not check GitHub releases. Check your connection and try again later.".into());
    }
    let info: Info = serde_json::from_slice(&output.stdout).map_err(|_| "Invalid release response.".to_string())?;
    if info.url != "https://github.com/kuaan08/Switchrecompz/releases/tag/dev" {
        return Err("Unexpected update source.".into());
    }
    Ok(info)
}
