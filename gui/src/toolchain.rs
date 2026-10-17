//! Finds the SwitchRecomp folder, Python and the C compilers, the same way
//! the bundled release is laid out (python\, tcc\, gcc\ next to the scripts).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const EXE: &str = std::env::consts::EXE_SUFFIX;

/// A way to start Python 3: the program plus arguments that go before the script.
#[derive(Clone, Debug)]
pub struct Python {
    pub program: PathBuf,
    pub args: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Toolchain {
    /// Folder that holds switchrecomp.py.
    pub home: Option<PathBuf>,
    pub python: Option<Python>,
    pub tcc: Option<PathBuf>,
    pub gcc: Option<PathBuf>,
}

/// Version strings, probed in the background because it starts processes.
#[derive(Clone, Debug, Default)]
pub struct Versions {
    pub python: Option<String>,
    pub tcc: Option<String>,
    pub gcc: Option<String>,
}

impl Toolchain {
    /// Tools next to the exe (release folder) or in a parent folder (development).
    pub fn detect() -> Self {
        Self::at(find_home())
    }

    /// Tools in `home`, e.g. where the single-file build unpacked itself.
    pub fn at(home: Option<PathBuf>) -> Self {
        let Some(home) = home.filter(|h| h.join("switchrecomp.py").is_file()) else {
            return Self::default();
        };
        Self { python: find_python(&home), tcc: find_tcc(&home), gcc: find_gcc(&home), home: Some(home) }
    }

    pub fn hactool(&self) -> Option<PathBuf> {
        self.home
            .as_ref()
            .map(|h| h.join("tools").join("hactool").join(format!("hactool{EXE}")))
            .filter(|p| p.is_file())
    }

    pub fn script(&self) -> Option<PathBuf> {
        self.home.as_ref().map(|h| h.join("switchrecomp.py"))
    }

    pub fn probe_versions(&self) -> Versions {
        Versions {
            python: self.python.as_ref().and_then(|py| {
                let mut args = py.args.clone();
                args.push("--version".into());
                first_line(&py.program, &args).map(|v| v.trim_start_matches("Python ").to_string())
            }),
            tcc: self.tcc.as_ref().and_then(|cc| {
                // "tcc version 0.9.27 (x86_64 Windows)"
                first_line(cc, &["-v".to_string()]).and_then(|v| v.split_whitespace().nth(2).map(str::to_string))
            }),
            gcc: self.gcc.as_ref().and_then(|cc| first_line(cc, &["-dumpversion".to_string()])),
        }
    }
}

/// The exe normally sits next to switchrecomp.py; during development it is
/// deeper inside gui\target\, so walk up from there (and from the cwd).
fn find_home() -> Option<PathBuf> {
    let starts = [std::env::current_exe().ok(), std::env::current_dir().ok()];
    starts
        .into_iter()
        .flatten()
        .flat_map(|p| p.ancestors().map(Path::to_path_buf).collect::<Vec<_>>())
        .find(|dir| dir.join("switchrecomp.py").is_file())
}

fn find_python(home: &Path) -> Option<Python> {
    let bundled = home.join("python").join(format!("python{EXE}"));
    if bundled.is_file() {
        return Some(Python { program: bundled, args: vec![] });
    }
    if cfg!(windows)
        && let Some(py) = which("py")
    {
        return Some(Python { program: py, args: vec!["-3".into()] });
    }
    ["python3", "python"].iter().filter_map(|n| which(n)).map(|program| Python { program, args: vec![] }).next()
}

fn find_tcc(home: &Path) -> Option<PathBuf> {
    first_file(&[home.join("tcc").join(format!("tcc{EXE}"))]).or_else(|| which("tcc"))
}

fn find_gcc(home: &Path) -> Option<PathBuf> {
    let mingw = format!("x86_64-w64-mingw32-gcc{EXE}");
    let gcc = format!("gcc{EXE}");
    first_file(&[
        home.join("gcc").join("Library").join("bin").join(&mingw),
        home.join("gcc").join("bin").join(&mingw),
        home.join("gcc").join("bin").join(&gcc),
        home.join("w64devkit").join("bin").join(&gcc),
    ])
    .or_else(|| if cfg!(windows) { which("x86_64-w64-mingw32-gcc") } else { None })
    .or_else(|| which("gcc"))
}

fn first_file(cands: &[PathBuf]) -> Option<PathBuf> {
    cands.iter().find(|p| p.is_file()).cloned()
}

/// Looks a program up on PATH. Skips the Microsoft Store "python.exe" stubs,
/// which open the Store instead of running anything.
pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(format!("{name}{EXE}")))
        .filter(|p| !p.to_string_lossy().to_ascii_lowercase().contains("\\windowsapps\\"))
        .find(|p| p.is_file())
}

fn first_line(program: &Path, args: &[String]) -> Option<String> {
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::null());
    hide_console(&mut cmd);
    let out = cmd.output().ok()?;
    // tcc and old pythons print their version on stderr
    let text = if out.stdout.is_empty() { out.stderr } else { out.stdout };
    String::from_utf8_lossy(&text).lines().next().map(|l| l.trim().to_string()).filter(|l| !l.is_empty())
}

/// PATH with `dirs` in front, so a bundled compiler finds its own helper
/// programs and DLLs (cc1, as, ld...).
pub fn path_with(dirs: &[&Path]) -> OsString {
    let mut parts: Vec<PathBuf> = dirs.iter().map(|d| d.to_path_buf()).collect();
    if let Some(path) = std::env::var_os("PATH") {
        parts.extend(std::env::split_paths(&path));
    }
    std::env::join_paths(parts).unwrap_or_default()
}

/// What to do about a missing tool. The Windows release carries Python and
/// GCC; on Linux they come from the system.
pub const PYTHON_HINT: &str = if cfg!(windows) {
    "Use the complete release (python folder) or install Python 3."
} else {
    "Install python3 with your package manager."
};
pub const GCC_HINT: &str = if cfg!(windows) {
    "Unzip the GCC package into the gcc folder, or put MinGW-w64 GCC on PATH."
} else {
    "Install gcc with your package manager."
};

/// Console programs started from a GUI app would flash a console window.
pub fn hide_console(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}
