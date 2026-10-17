//! GUI choices remembered between runs, stored as SwitchRecomp.json next to
//! switchrecomp.py (the release is portable, like the games it writes).

use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

const MAX_RECENT: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Compiler {
    Tcc,
    Gcc,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub input: String,
    pub hactool: String,
    pub prod_keys: String,
    pub title_keys: String,
    pub ticket: String,
    pub name: String,
    /// Empty means the games folder next to switchrecomp.py.
    pub out_dir: String,
    pub compiler: Compiler,
    pub android: bool,
    pub android_sdk: String,
    pub android_ndk: String,
    pub renderer: String,
    pub opt_level: u8,
    pub native: bool,
    /// 0 means one job per CPU thread.
    pub jobs: usize,
    pub interp_only: bool,
    pub recent: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            input: String::new(),
            hactool: String::new(),
            prod_keys: String::new(),
            title_keys: String::new(),
            ticket: String::new(),
            name: String::new(),
            out_dir: String::new(),
            compiler: Compiler::Tcc,
            android: false,
            android_sdk: String::new(),
            android_ndk: String::new(),
            renderer: "opengl".into(),
            opt_level: 2,
            native: false,
            jobs: 0,
            interp_only: false,
            recent: Vec::new(),
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        let text = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }

    pub fn remember_input(&mut self, input: &str) {
        self.recent.retain(|r| r != input);
        self.recent.insert(0, input.to_string());
        self.recent.truncate(MAX_RECENT);
    }
}
