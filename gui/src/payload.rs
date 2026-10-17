//! The single-file build carries the scripts, the runtimes, Python, TCC and
//! GCC inside the exe (packed by build.rs) and unpacks them on first start,
//! once per build, to %LOCALAPPDATA%\SwitchRecomp\<build id>.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

#[cfg(payload)]
static ARCHIVE: &[u8] = include_bytes!(env!("SWR_PAYLOAD_FILE"));
#[cfg(not(payload))]
static ARCHIVE: &[u8] = &[];

/// Marks a folder that was unpacked completely.
const MARKER: &str = ".unpacked";

pub fn embedded() -> bool {
    !ARCHIVE.is_empty()
}

/// Size of the packed data, for progress reporting.
pub fn packed_len() -> u64 {
    ARCHIVE.len() as u64
}

fn build_id() -> &'static str {
    option_env!("SWR_PAYLOAD_ID").unwrap_or("none")
}

/// %LOCALAPPDATA%\SwitchRecomp (or ~/.local/share/SwitchRecomp).
pub fn cache_root() -> Option<PathBuf> {
    #[cfg(windows)]
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")));
    base.map(|b| b.join("SwitchRecomp"))
}

/// Where this build's files are (or will be) unpacked.
pub fn home() -> Option<PathBuf> {
    cache_root().map(|r| r.join(build_id()))
}

pub fn ready() -> bool {
    home().is_some_and(|h| h.join(MARKER).is_file())
}

/// Unpacks into the cache unless an earlier start already did, and returns
/// the folder. `done` counts packed bytes read, for a progress bar.
pub fn unpack_cached(done: &AtomicU64) -> io::Result<PathBuf> {
    let home = home().ok_or_else(|| io::Error::other("no local app data folder"))?;
    if home.join(MARKER).is_file() {
        return Ok(home);
    }
    let root = home.parent().expect("cache root").to_path_buf();
    // unpack next to the target and rename it in place, so a crash never
    // leaves a half-filled folder that looks complete
    let tmp = root.join(format!("{}.partial-{}", build_id(), std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    unpack_to(&tmp, done)?;
    fs::write(tmp.join(MARKER), build_id())?;
    if home.join(MARKER).is_file() {
        // another instance finished first
        let _ = fs::remove_dir_all(&tmp);
    } else {
        if home.exists() {
            fs::remove_dir_all(&home)?;
        }
        if let Err(e) = fs::rename(&tmp, &home) {
            if !home.join(MARKER).is_file() {
                return Err(e);
            }
            let _ = fs::remove_dir_all(&tmp);
        }
    }
    remove_old(&root);
    Ok(home)
}

/// Unpacks everything into `dir` (also used by `--unpack DIR`).
pub fn unpack_to(dir: &Path, done: &AtomicU64) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let mut dec = zstd::stream::read::Decoder::new(Counted { data: ARCHIVE, done })?;
    dec.window_log_max(27)?;
    let mut archive = tar::Archive::new(dec);
    for entry in archive.entries()? {
        entry?.unpack_in(dir)?;
    }
    Ok(())
}

/// Older builds' folders, and unpacks interrupted more than a day ago.
fn remove_old(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else { return };
    let day_ago = SystemTime::now() - Duration::from_secs(86_400);
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let old_build = name.len() == 16 && name != build_id() && name.chars().all(|c| c.is_ascii_hexdigit());
        let stale_partial =
            name.contains(".partial-") && e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t < day_ago);
        if old_build || stale_partial {
            let _ = fs::remove_dir_all(e.path());
        }
    }
}

struct Counted<'a> {
    data: &'a [u8],
    done: &'a AtomicU64,
}

impl Read for Counted<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.data.read(buf)?;
        self.done.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}
