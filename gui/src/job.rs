//! Runs switchrecomp.py in the background, streams its output and turns the
//! progress lines it prints into a phase + fraction for the progress bar.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Instant;

use eframe::egui;

use crate::toolchain::{self, Python};

pub struct JobConfig {
    pub python: Python,
    pub home: PathBuf,
    pub script: PathBuf,
    pub input: PathBuf,
    pub hactool: String,
    pub prod_keys: String,
    pub title_keys: String,
    pub out_dir: PathBuf,
    pub name: Option<String>,
    pub cc: PathBuf,
    pub gcc: bool,
    pub opt_level: u8,
    pub native: bool,
    pub jobs: usize,
    pub interp_only: bool,
}

impl JobConfig {
    fn args(&self) -> Vec<OsString> {
        let mut a: Vec<OsString> = self.python.args.iter().map(OsString::from).collect();
        // unbuffered so progress arrives line by line, UTF-8 whatever the console code page
        a.extend(["-u", "-X", "utf8"].map(OsString::from));
        a.push(self.script.clone().into());
        a.push(self.input.clone().into());
        for (flag, value) in [
            ("--hactool", &self.hactool),
            ("--keys", &self.prod_keys),
            ("--title-keys", &self.title_keys),
        ] {
            if !value.trim().is_empty() {
                a.push(flag.into());
                a.push(value.trim().into());
            }
        }
        if cfg!(windows) {
            a.extend(["--target", "win64"].map(OsString::from));
        }
        a.push("--outdir".into());
        a.push(self.out_dir.clone().into());
        a.push("--cc".into());
        a.push(self.cc.clone().into());
        a.push("--jobs".into());
        a.push(self.jobs.to_string().into());
        if self.gcc {
            a.push("-O".into());
            a.push(self.opt_level.to_string().into());
            if self.native {
                a.push("--native".into());
            }
        }
        if let Some(name) = &self.name {
            a.push("-o".into());
            a.push(name.into());
        }
        if self.interp_only {
            a.push("--interp-only".into());
        }
        a
    }

    /// The command line as the user could type it.
    pub fn display(&self) -> String {
        std::iter::once(self.python.program.clone().into_os_string())
            .chain(self.args())
            .map(|a| {
                let s = a.to_string_lossy().into_owned();
                if s.is_empty() || s.contains(' ') { format!("\"{s}\"") } else { s }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(&self.python.program);
        cmd.args(self.args()).current_dir(&self.home).env("PYTHONIOENCODING", "utf-8");
        if let Some(dir) = self.cc.parent() {
            cmd.env("PATH", toolchain::path_with(&[dir]));
        }
        toolchain::hide_console(&mut cmd);
        #[cfg(unix)]
        {
            // own process group, so cancel can stop the compilers too
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        cmd
    }
}

pub enum JobEvent {
    Line(String),
    Exited(Option<i32>),
}

pub struct Job {
    rx: Receiver<JobEvent>,
    pid: u32,
    pub started: Instant,
    pub cancelled: bool,
}

impl Job {
    pub fn start(cfg: &JobConfig, ctx: &egui::Context) -> std::io::Result<Job> {
        let mut child = cfg.command().stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
        let (tx, rx) = mpsc::channel();
        let readers = [
            child.stdout.take().map(|s| read_lines(s, tx.clone(), ctx.clone())),
            child.stderr.take().map(|s| read_lines(s, tx.clone(), ctx.clone())),
        ];
        let pid = child.id();
        let ctx = ctx.clone();
        thread::spawn(move || {
            for r in readers.into_iter().flatten() {
                let _ = r.join();
            }
            let code = child.wait().ok().and_then(|s| s.code());
            let _ = tx.send(JobEvent::Exited(code));
            ctx.request_repaint();
        });
        Ok(Job { rx, pid, started: Instant::now(), cancelled: false })
    }

    pub fn events(&self) -> Vec<JobEvent> {
        self.rx.try_iter().collect()
    }

    /// Stops Python and the compilers it started.
    pub fn cancel(&mut self) {
        if !self.cancelled {
            self.cancelled = true;
            kill_tree(self.pid);
        }
    }
}

fn read_lines<R: Read + Send + 'static>(src: R, tx: Sender<JobEvent>, ctx: egui::Context) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut reader = BufReader::new(src);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let line = String::from_utf8_lossy(&buf).trim_end_matches(['\r', '\n']).to_string();
                    if tx.send(JobEvent::Line(line)).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            }
        }
    })
}

fn kill_tree(pid: u32) {
    #[cfg(windows)]
    let mut cmd = {
        let mut c = Command::new("taskkill");
        c.args(["/PID", &pid.to_string(), "/T", "/F"]);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = Command::new("kill");
        c.args(["-KILL", &format!("-{pid}")]);
        c
    };
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    toolchain::hide_console(&mut cmd);
    let _ = cmd.status();
}

// ------------------------------------------------------------------ progress

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Starting,
    Analyzing,
    Translating,
    Copying,
    Compiling,
    Linking,
    Runtime,
    Done,
    Failed,
    Cancelled,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Phase::Starting => "Loading the game",
            Phase::Analyzing => "Analyzing the code",
            Phase::Translating => "Translating ARM64 to C",
            Phase::Copying => "Copying game data",
            Phase::Compiling => "Compiling",
            Phase::Linking => "Linking",
            Phase::Runtime => "Building the runtime",
            Phase::Done => "Done",
            Phase::Failed => "Failed",
            Phase::Cancelled => "Cancelled",
        }
    }

    /// Share of the whole run, roughly what each phase takes on a big game.
    fn span(self) -> (f32, f32) {
        match self {
            Phase::Starting => (0.0, 0.03),
            Phase::Analyzing => (0.03, 0.15),
            Phase::Translating => (0.15, 0.35),
            Phase::Copying => (0.35, 0.40),
            Phase::Compiling => (0.40, 0.95),
            Phase::Linking | Phase::Runtime => (0.95, 0.99),
            Phase::Done | Phase::Failed | Phase::Cancelled => (1.0, 1.0),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Progress {
    pub phase: Phase,
    pub step: Option<(u32, u32)>,
    /// From the final "Done: <path>" line.
    pub exe: Option<PathBuf>,
}

impl Default for Progress {
    fn default() -> Self {
        Self { phase: Phase::Starting, step: None, exe: None }
    }
}

impl Progress {
    pub fn feed(&mut self, line: &str) {
        let t = line.trim();
        let phase = if t.starts_with("Analyzing") {
            Some(Phase::Analyzing)
        } else if t.starts_with("Translating") {
            Some(Phase::Translating)
        } else if t.starts_with("Copying romfs") {
            Some(Phase::Copying)
        } else if t.starts_with("Compiling the runtime") {
            Some(Phase::Runtime)
        } else if t.starts_with("Compiling with") {
            Some(Phase::Compiling)
        } else if t.starts_with("Linking") {
            Some(Phase::Linking)
        } else {
            None
        };
        if let Some(p) = phase {
            self.phase = p;
            self.step = None;
        } else if let Some(rest) = t.strip_prefix("C generated:").or_else(|| t.strip_prefix("compiled ")) {
            self.step = parse_ratio(rest).or(self.step);
        } else if let Some(path) = t.strip_prefix("Done: ") {
            self.exe = Some(PathBuf::from(path));
        }
    }

    pub fn fraction(&self) -> f32 {
        let (a, b) = self.phase.span();
        let f = self.step.map_or(0.0, |(x, n)| x as f32 / n.max(1) as f32);
        a + (b - a) * f.clamp(0.0, 1.0)
    }

    pub fn describe(&self) -> String {
        match (self.phase, self.step) {
            (Phase::Translating, Some((x, n))) => format!("{} ({x}/{n} files)", self.phase.label()),
            (Phase::Compiling, Some((x, n))) => format!("{} ({x}/{n} files)", self.phase.label()),
            _ => self.phase.label().to_string(),
        }
    }
}

fn parse_ratio(s: &str) -> Option<(u32, u32)> {
    let (a, b) = s.split_whitespace().next()?.split_once('/')?;
    Some((a.parse().ok()?, b.parse().ok()?))
}

// ------------------------------------------------------------------ log

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Command,
    Info,
    Detail,
    Warning,
    Error,
    Success,
}

pub fn classify(line: &str) -> LineKind {
    let t = line.trim_start();
    let lower = t.to_ascii_lowercase();
    if line.starts_with("> ") {
        LineKind::Command
    } else if t.starts_with("Done:") {
        LineKind::Success
    } else if t.starts_with("Traceback")
        || lower.contains("error")
        || lower.contains("failed")
        || lower.contains("cannot ")
        || lower.contains("not found")
        || lower.starts_with("no c compiler")
    {
        LineKind::Error
    } else if lower.contains("warning") {
        LineKind::Warning
    } else if line.starts_with(' ') {
        LineKind::Detail
    } else {
        LineKind::Info
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn follows_the_script_output() {
        let mut p = Progress::default();
        for l in [
            "SwitchRecomp - use your own copy of the game. No games or keys included.",
            "Analyzing the code...",
            "Translating ARM64 to C...",
            "  C generated: 21/57 chunks (3s)",
        ] {
            p.feed(l);
        }
        assert_eq!(p.phase, Phase::Translating);
        assert_eq!(p.step, Some((21, 57)));
        p.feed("Compiling with tcc.exe (-O2, 8 at a time)...");
        assert_eq!((p.phase, p.step), (Phase::Compiling, None));
        p.feed("  compiled 10/58 (2s)");
        assert_eq!(p.step, Some((10, 58)));
        p.feed("Done: C:\\games\\My Game.exe");
        assert_eq!(p.exe.as_deref(), Some(Path::new("C:\\games\\My Game.exe")));
    }

    #[test]
    fn classifies_lines() {
        assert_eq!(classify("Compilation failed:"), LineKind::Error);
        assert_eq!(classify("  compiled 10/58 (2s)"), LineKind::Detail);
        assert_eq!(classify("Done: x.exe"), LineKind::Success);
        assert_eq!(classify("Linking x_code.dll ..."), LineKind::Info);
    }
}
