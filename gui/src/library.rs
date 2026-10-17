//! The recompiled games in the output folder: <Name>.exe + <Name>_code.dll
//! (+ .ini, .log, _data\, _build\).

use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

const CODE_SUFFIXES: [&str; 2] = ["_code.dll", "_code.so"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeCompiler {
    Tcc,
    Gcc,
    Unknown,
}

impl CodeCompiler {
    pub fn label(self) -> &'static str {
        match self {
            CodeCompiler::Tcc => "TCC",
            CodeCompiler::Gcc => "GCC",
            CodeCompiler::Unknown => "?",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Game {
    pub name: String,
    pub dir: PathBuf,
    pub exe: PathBuf,
    pub built: Option<SystemTime>,
    pub code_size: u64,
    pub compiler: CodeCompiler,
    pub build_cache: u64,
}

impl Game {
    pub fn ini(&self) -> PathBuf {
        self.dir.join(format!("{}.ini", self.name))
    }
    pub fn log(&self) -> PathBuf {
        self.dir.join(format!("{}.log", self.name))
    }
    pub fn data_dir(&self) -> PathBuf {
        self.dir.join(format!("{}_data", self.name))
    }
    pub fn build_dir(&self) -> PathBuf {
        self.dir.join(format!("{}_build", self.name))
    }
}

/// Newest first.
pub fn scan(dir: &Path) -> Vec<Game> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut games: Vec<Game> = entries
        .flatten()
        .filter_map(|e| {
            let file = e.file_name().to_string_lossy().into_owned();
            let name = CODE_SUFFIXES.iter().find_map(|s| file.strip_suffix(s))?.to_string();
            let exe = dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
            if !exe.is_file() {
                return None;
            }
            let code = e.path();
            let meta = e.metadata().ok();
            let build_cache = dir_size(&dir.join(format!("{name}_build")));
            Some(Game {
                compiler: code_compiler(&code),
                built: meta.as_ref().and_then(|m| m.modified().ok()),
                code_size: meta.map_or(0, |m| m.len()),
                name,
                dir: dir.to_path_buf(),
                exe,
                build_cache,
            })
        })
        .collect();
    games.sort_by_key(|g| std::cmp::Reverse(g.built));
    games
}

/// The runtime tells TCC from GCC builds by the PE linker version
/// (tcc writes 6.0, GNU ld writes 2.x); do the same.
fn code_compiler(path: &Path) -> CodeCompiler {
    let mut head = [0u8; 0x200];
    let ok = File::open(path).and_then(|mut f| f.read_exact(&mut head)).is_ok();
    if !ok || &head[..2] != b"MZ" {
        return CodeCompiler::Unknown;
    }
    let pe = u32::from_le_bytes([head[0x3C], head[0x3D], head[0x3E], head[0x3F]]) as usize;
    match head.get(pe..pe + 28) {
        Some(h) if &h[..4] == b"PE\0\0" => {
            if (h[26], h[27]) == (6, 0) {
                CodeCompiler::Tcc
            } else {
                CodeCompiler::Gcc
            }
        }
        _ => CodeCompiler::Unknown,
    }
}

/// Size of the files directly inside `dir` (the build folder is flat).
fn dir_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|it| it.flatten().filter_map(|e| e.metadata().ok()).filter(|m| m.is_file()).map(|m| m.len()).sum())
        .unwrap_or(0)
}

pub fn launch(exe: &Path) -> io::Result<()> {
    // the runtime writes captures\ and performance.txt to the working folder
    let mut cmd = Command::new(exe);
    if let Some(dir) = exe.parent() {
        cmd.current_dir(dir);
    }
    cmd.spawn().map(|_| ())
}

/// Opens a folder, or a file with its default program.
pub fn open(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    let mut cmd = Command::new("explorer");
    #[cfg(target_os = "macos")]
    let mut cmd = Command::new("open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = Command::new("xdg-open");
    cmd.arg(path).spawn().map(|_| ())
}

/// Opens the folder that contains `path` with the file selected.
pub fn reveal(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // explorer wants the quotes around the path only, not around /select
        Command::new("explorer").raw_arg(format!("/select,\"{}\"", path.display())).spawn().map(|_| ())
    }
    #[cfg(not(windows))]
    {
        open(path.parent().unwrap_or(path))
    }
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 { format!("{bytes} B") } else { format!("{v:.1} {}", UNITS[u]) }
}

pub fn time_ago(t: SystemTime) -> String {
    let secs = SystemTime::now().duration_since(t).map_or(0, |d| d.as_secs());
    let plural = |n: u64, unit: &str| format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" });
    match secs {
        0..60 => "just now".into(),
        60..3600 => plural(secs / 60, "minute"),
        3600..86400 => plural(secs / 3600, "hour"),
        86400..172800 => "yesterday".into(),
        172800..2592000 => plural(secs / 86400, "day"),
        2592000..31536000 => plural(secs / 2592000, "month"),
        _ => plural(secs / 31536000, "year"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(5 * 1024 * 1024 * 1024), "5.0 GB");
    }
}
