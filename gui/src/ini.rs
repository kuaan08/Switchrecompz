//! Per-game settings (<Name>.ini). The schema and help texts mirror
//! INI_SECS in runtime/main.c, and files are written in the same layout the
//! runtime rewrites on every start, so both stay interchangeable. A running
//! game notices the change and applies most settings live.

use std::io;
use std::path::{Path, PathBuf};

pub enum Kind {
    Bool,
    Choice(&'static [(&'static str, &'static str)]),
    Float { min: f32, max: f32, step: f32 },
    Int { min: i32, max: i32 },
    File { filter: (&'static str, &'static [&'static str]) },
}

pub struct Opt {
    pub key: &'static str,
    pub default: &'static str,
    pub label: &'static str,
    /// Same text as the runtime writes above the key.
    pub help: &'static str,
    pub kind: Kind,
}

pub struct Section {
    pub title: &'static str,
    /// Features the Linux runtime doesn't have (Lossless Scaling, F7 recording)
    pub windows_only: bool,
    pub opts: &'static [Opt],
}

const LANGUAGES: &[(&str, &str)] = &[
    ("0", "Japanese"),
    ("1", "English (US)"),
    ("2", "French"),
    ("3", "German"),
    ("4", "Italian"),
    ("5", "Spanish"),
    ("6", "Chinese"),
    ("7", "Korean"),
    ("8", "Dutch"),
    ("9", "Portuguese"),
    ("10", "Russian"),
    ("11", "Taiwanese"),
    ("12", "English (UK)"),
    ("13", "French (Canada)"),
    ("14", "Spanish (Latin America)"),
];

pub const SECTIONS: &[Section] = &[
    Section {
        title: "Display",
        windows_only: false,
        opts: &[
            Opt {
                key: "docked",
                default: "0",
                label: "TV mode (docked)",
                help: "1 = TV mode (1920x1080 / 1600x900 games), 0 = handheld mode (1280x720)",
                kind: Kind::Bool,
            },
            Opt {
                key: "fullscreen",
                default: "1",
                label: "Start in fullscreen",
                help: "1 = start in fullscreen (F11 or Alt+Enter toggles it in game)",
                kind: Kind::Bool,
            },
            Opt {
                key: "language",
                default: "1",
                label: "Game language",
                help: "Game language: 0 Japanese, 1 English (US), 2 French, 3 German, 4 Italian, 5 Spanish,\n;   6 Chinese, 7 Korean, 8 Dutch, 9 Portuguese, 10 Russian, 11 Taiwanese, 12 English (UK),\n;   13 French (Canada), 14 Spanish (Latin America)",
                kind: Kind::Choice(LANGUAGES),
            },
        ],
    },
    Section {
        title: "Graphics",
        windows_only: false,
        opts: &[
            Opt {
                key: "resolution",
                default: "1",
                label: "Internal resolution",
                help: "Internal 3D resolution: 1 = native, 2 = double, 0.75 / 0.5 = faster (applies live)",
                kind: Kind::Float { min: 0.25, max: 4.0, step: 0.05 },
            },
            Opt {
                key: "dynamic_resolution",
                default: "0",
                label: "Dynamic resolution",
                help: "1 = lower the resolution automatically when the GPU can't keep up (down to dynamic_resolution_min)",
                kind: Kind::Bool,
            },
            Opt {
                key: "dynamic_resolution_min",
                default: "0.6",
                label: "Dynamic resolution minimum",
                help: "Lowest internal resolution allowed by dynamic_resolution",
                kind: Kind::Float { min: 0.25, max: 1.0, step: 0.05 },
            },
            Opt {
                key: "game_dynamic_resolution",
                default: "0",
                label: "Game's own dynamic resolution",
                help: "1 = let the game lower its own resolution like on the console (0 = always full resolution)",
                kind: Kind::Bool,
            },
            Opt {
                key: "shadows",
                default: "1",
                label: "Shadows",
                help: "0 = disable shadow maps (more FPS)",
                kind: Kind::Bool,
            },
        ],
    },
    Section {
        title: "Performance",
        windows_only: false,
        opts: &[
            Opt {
                key: "fps_limit",
                default: "1",
                label: "FPS limit",
                help: "1 = 60 FPS (the speed of a Switch), 0 = unlimited (the game runs faster). Values under 60 are treated as 60: the game advances one step per image, so 30 meant half speed. F4 changes it in game",
                kind: Kind::Choice(&[("1", "60 FPS (console speed)"), ("0", "Unlimited (game runs faster)")]),
            },
            Opt {
                key: "forced_vsync",
                default: "0",
                label: "Always wait for vsync",
                help: "1 = always wait for vsync (can lower FPS a lot on slow PCs)",
                kind: Kind::Bool,
            },
            Opt {
                key: "memory_gb",
                default: "0",
                label: "Game memory",
                help: "Memory the game sees: 0 = automatic (3.2 GB like the console, more for games that need it,\n;   e.g. Tears of the Kingdom), 4 = console (3.2 GB), 6 or 8 = extended (like yuzu's extended memory layout)",
                kind: Kind::Choice(&[
                    ("0", "Automatic"),
                    ("4", "Console (3.2 GB)"),
                    ("6", "Extended (6 GB)"),
                    ("8", "Extended (8 GB)"),
                ]),
            },
            Opt {
                key: "low_priority_shader_compiler",
                default: "1",
                label: "Low-priority shader compiler",
                help: "1 = compile shaders at low priority (less stutter on weak CPUs)",
                kind: Kind::Bool,
            },
        ],
    },
    Section {
        title: "Audio",
        windows_only: false,
        opts: &[Opt {
            key: "volume",
            default: "100",
            label: "Volume",
            help: "Game volume, 0 to 100 (0 = muted, applies live)",
            kind: Kind::Int { min: 0, max: 100 },
        }],
    },
    Section {
        title: "Frame generation",
        windows_only: true,
        opts: &[
            Opt {
                key: "lsfg",
                default: "1",
                label: "Frame generation (LSFG)",
                help: "In-game frame generation (LSFG). Uses the Lossless.dll from YOUR Lossless Scaling\n;   (copy it into this folder or set lsfg_dll). Only generates frames when the game is below lsfg_target",
                kind: Kind::Bool,
            },
            Opt {
                key: "lsfg_target",
                default: "60",
                label: "Target FPS",
                help: "Target FPS for frame generation",
                kind: Kind::Int { min: 30, max: 360 },
            },
            Opt {
                key: "lsfg_multiplier",
                default: "2",
                label: "Multiplier",
                help: "2 = x2, 3 = x3, 4 = x4",
                kind: Kind::Choice(&[("2", "x2"), ("3", "x3"), ("4", "x4")]),
            },
            Opt {
                key: "lsfg_flow",
                default: "0.5",
                label: "Flow scale",
                help: "Flow scale: 1 = best quality, 0.5 = faster (recommended on integrated GPUs)",
                kind: Kind::Float { min: 0.25, max: 1.0, step: 0.05 },
            },
            Opt {
                key: "lsfg_resolution",
                default: "720",
                label: "Resolution (height)",
                help: "Resolution (height) used for frame generation",
                kind: Kind::Int { min: 240, max: 2160 },
            },
            Opt {
                key: "lsfg_dll",
                default: "",
                label: "Lossless.dll",
                help: "Path to Lossless.dll (empty = search automatically)",
                kind: Kind::File { filter: ("Lossless.dll", &["dll"]) },
            },
            Opt {
                key: "lossless_scaling",
                default: "0",
                label: "Also launch Lossless Scaling",
                help: "1 = also launch the Lossless Scaling app with the game (not needed when using lsfg)",
                kind: Kind::Bool,
            },
            Opt {
                key: "lossless_path",
                default: "",
                label: "LosslessScaling.exe",
                help: "Path to LosslessScaling.exe (empty = search in Steam)",
                kind: Kind::File { filter: ("LosslessScaling.exe", &["exe"]) },
            },
        ],
    },
    Section {
        title: "Recording",
        windows_only: true,
        opts: &[
            Opt {
                key: "smooth_recording",
                default: "1",
                label: "Smooth recording",
                help: "1 = F7 recordings are always smooth: the game runs in slow motion while recording\n;   if the PC can't keep up, but the exported video plays at normal speed with no stutter",
                kind: Kind::Bool,
            },
            Opt {
                key: "recording_fps",
                default: "60",
                label: "Video FPS",
                help: "FPS of the exported video (constant)",
                kind: Kind::Int { min: 15, max: 60 },
            },
        ],
    },
    Section {
        title: "Debug",
        windows_only: false,
        opts: &[
            Opt {
                key: "log",
                default: "1",
                label: "Log detail",
                help: "Log detail: 1 = normal, 2 = everything (slow)",
                kind: Kind::Choice(&[("1", "Normal"), ("2", "Everything (slow)")]),
            },
            Opt {
                key: "diag",
                default: "0",
                label: "Automatic diagnostic dumps",
                help: "1 = automatic diagnostic dumps (F5 always works)",
                kind: Kind::Bool,
            },
            Opt {
                key: "profile",
                default: "0",
                label: "CPU profiler",
                help: "1 = write profile.txt every 10 seconds (CPU profiler)",
                kind: Kind::Bool,
            },
        ],
    },
];

fn known(key: &str) -> bool {
    SECTIONS.iter().flat_map(|s| s.opts).any(|o| o.key == key)
}

/// The values of one game's .ini, in file order.
#[derive(Clone, Debug)]
pub struct GameIni {
    pub path: PathBuf,
    pub values: Vec<(String, String)>,
}

impl GameIni {
    /// Parses like ini_load() in runtime/main.c. A missing file is empty.
    pub fn load(path: &Path) -> Self {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let mut ini = GameIni { path: path.to_path_buf(), values: Vec::new() };
        for line in text.lines() {
            if line.starts_with([';', '#', '[']) {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else { continue };
            let k = k.trim();
            if !k.is_empty() && k != "vsync" {
                ini.set(k, v.trim());
            }
        }
        ini
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    pub fn value(&self, opt: &Opt) -> String {
        self.get(opt.key).unwrap_or(opt.default).to_string()
    }

    pub fn set(&mut self, key: &str, value: &str) {
        match self.values.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value.to_string(),
            None => self.values.push((key.to_string(), value.to_string())),
        }
    }

    pub fn others(&self) -> impl Iterator<Item = &(String, String)> {
        self.values.iter().filter(|(k, _)| !known(k))
    }

    pub fn reset(&mut self) {
        self.values.retain(|(k, _)| !known(k));
    }

    /// Same layout as ini_write() in runtime/main.c.
    pub fn save(&self, title: &str) -> io::Result<()> {
        let mut out = String::new();
        out.push_str("; =====================================================================\n");
        out.push_str(&format!(";  {title} - SwitchRecomp settings\n"));
        out.push_str(";  Edit a value and save: most settings apply live while the game runs\n");
        out.push_str(";  (docked, language, fullscreen and frame generation apply on restart).\n");
        out.push_str(";  Keys in game: F1 help, F3 screenshot, F4 FPS limit, F5 trace,\n");
        out.push_str(";  F7 record video, F8 full-res screenshot, F11 fullscreen.\n");
        out.push_str("; =====================================================================\n");
        for s in SECTIONS {
            out.push_str(&format!("\n[{}]\n", s.title));
            for o in s.opts {
                out.push_str(&format!("; {}\n{}={}\n", o.help, o.key, self.value(o)));
            }
        }
        let mut others = self.others().peekable();
        if others.peek().is_some() {
            out.push_str("\n[Other]\n");
            for (k, v) in others {
                out.push_str(&format!("{k}={v}\n"));
            }
        }
        // the runtime writes in text mode, so CRLF on Windows
        if cfg!(windows) {
            out = out.replace('\n', "\r\n");
        }
        let tmp = self.path.with_extension("ini.tmp");
        std::fs::write(&tmp, out)?;
        std::fs::rename(&tmp, &self.path)
    }
}

/// Floats are written short ("0.75", "1") like a person would type them.
pub fn format_float(v: f32) -> String {
    let r = (v * 100.0).round() / 100.0;
    format!("{r}")
}

/// atoi() semantics, as the runtime reads flags.
pub fn as_bool(v: &str) -> bool {
    let digits: String = v.trim().chars().take_while(|c| c.is_ascii_digit() || *c == '-').collect();
    digits.parse::<i64>().map(|n| n != 0).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_the_runtime_layout() {
        let dir = std::env::temp_dir().join(format!("swr-ini-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Game.ini");
        std::fs::write(&path, "[Display]\nlanguage = 2\n; comment=no\nseconds=5\nvsync=1\n").unwrap();
        let mut ini = GameIni::load(&path);
        assert_eq!(ini.get("language"), Some("2"));
        assert_eq!(ini.get("vsync"), None);
        ini.set("resolution", &format_float(0.7000001));
        ini.save("Game").unwrap();
        let again = GameIni::load(&path);
        assert_eq!(again.get("resolution"), Some("0.7"));
        assert_eq!(again.get("docked"), Some("0"));
        assert_eq!(again.get("seconds"), Some("5"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn flags_follow_atoi() {
        assert!(as_bool("1"));
        assert!(as_bool("2 "));
        assert!(!as_bool("0"));
        assert!(!as_bool("yes"));
    }
}
