//! Quick look at the selected input (game folder or .nro) so problems show up
//! before a recompile starts. Mirrors what nxloader.py accepts.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Same load order as nxloader.MODULE_ORDER.
const MODULES: [&str; 13] = [
    "rtld", "main", "subsdk0", "subsdk1", "subsdk2", "subsdk3", "subsdk4", "subsdk5", "subsdk6", "subsdk7", "subsdk8",
    "subsdk9", "sdk",
];

#[derive(Clone, Debug)]
pub enum Kind {
    Folder { romfs: Option<PathBuf> },
    Nro,
    Package,
}

#[derive(Clone, Debug)]
pub struct GameInfo {
    pub kind: Kind,
    /// Display name from main.npdm or the NRO's NACP.
    pub name: Option<String>,
    pub title_id: Option<u64>,
    /// (module name, size in bytes)
    pub modules: Vec<(String, u64)>,
    /// Output name switchrecomp.py picks when no name is given.
    pub default_title: String,
    pub warnings: Vec<String>,
}

pub fn inspect(path: &Path) -> Result<GameInfo, String> {
    if path.as_os_str().is_empty() {
        return Err("No game selected.".into());
    }
    if path.is_file() {
        let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        return match ext.as_str() {
            "nro" => inspect_nro(path),
            "nsp" | "xci" | "nca" => {
                if ext == "nsp" && magic(path, 0) != Some(*b"PFS0") {
                    return Err("Invalid NSP header (expected PFS0).".into());
                }
                if ext == "xci" && magic(path, 0x100) != Some(*b"HEAD") {
                    return Err("Invalid XCI header (expected HEAD).".into());
                }
                let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                Ok(GameInfo {
                    kind: Kind::Package,
                    name: None,
                    title_id: None,
                    modules: vec![],
                    default_title: default_title(None, &stem),
                    warnings: vec![
                        "Package selected; contents will be verified during import. Configure your own hactool and keys below. Import support does not guarantee game compatibility.".into(),
                    ],
                })
            }
            "nsz" | "xcz" => Err("Compressed packages are unsupported; decompress your own dump first.".into()),
            _ => Err("Pick a game folder, .nro, .nsp, .xci or .nca file.".into()),
        };
    }
    if !path.is_dir() {
        return Err("This path does not exist.".into());
    }
    inspect_folder(path)
}

fn inspect_folder(dir: &Path) -> Result<GameInfo, String> {
    let exefs = [dir.join("exefs"), dir.join("ExeFS"), dir.to_path_buf()]
        .into_iter()
        .find(|c| c.join("main").is_file())
        .ok_or("No exefs\\main found. Pick the folder that contains exefs and romfs.")?;

    let mut modules = Vec::new();
    let mut warnings = Vec::new();
    for name in MODULES {
        let p = exefs.join(name);
        if !p.is_file() {
            continue;
        }
        if magic(&p, 0) != Some(*b"NSO0") {
            if name == "main" {
                return Err("exefs\\main is not an NSO executable (still encrypted or damaged?).".into());
            }
            warnings.push(format!("exefs\\{name} is not an NSO file and will fail to load."));
        }
        modules.push((name.to_string(), file_size(&p)));
    }

    let (name, title_id) = read_npdm(&exefs.join("main.npdm"));
    if title_id.is_none() {
        warnings.push("No valid exefs\\main.npdm: default stack size and priority will be used.".into());
    }

    let root = if exefs.file_name().is_some_and(|n| n.eq_ignore_ascii_case("exefs")) {
        exefs.parent().unwrap_or(dir).to_path_buf()
    } else {
        exefs.clone()
    };
    let romfs = [root.join("romfs"), root.join("RomFS")].into_iter().find(|p| p.is_dir());
    if romfs.is_none() {
        warnings.push("No romfs folder next to exefs: most games need their romfs to run.".into());
    }

    let fallback = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(GameInfo {
        default_title: default_title(name.as_deref(), &fallback),
        kind: Kind::Folder { romfs },
        name,
        title_id,
        modules,
        warnings,
    })
}

fn inspect_nro(path: &Path) -> Result<GameInfo, String> {
    let data = std::fs::read(path).map_err(|e| format!("Cannot read the file: {e}"))?;
    if data.get(0x10..0x14) != Some(b"NRO0") {
        return Err("This .nro file is not a valid NRO executable.".into());
    }
    let stem = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(GameInfo {
        kind: Kind::Nro,
        name: nacp_name(&data),
        title_id: None,
        modules: vec![("main".into(), data.len() as u64)],
        // homebrew has no npdm, so switchrecomp.py names it after the file
        default_title: default_title(None, &stem),
        warnings: vec![],
    })
}

/// main.npdm: name at 0x20, ACI0 offset at 0x70, program id at ACI0+0x10.
fn read_npdm(path: &Path) -> (Option<String>, Option<u64>) {
    let Ok(d) = std::fs::read(path) else { return (None, None) };
    if d.len() < 0x80 || &d[..4] != b"META" {
        return (None, None);
    }
    let name = cstr_latin1(&d[0x20..0x30]);
    let aci = u32_at(&d, 0x70).unwrap_or(0) as usize;
    let title_id = (d.get(aci..aci + 4) == Some(b"ACI0")).then(|| u64_at(&d, aci + 0x10)).flatten();
    (Some(name).filter(|n| !n.is_empty()), title_id)
}

/// Title of the first language entry that has one, from the NRO asset section.
fn nacp_name(d: &[u8]) -> Option<String> {
    let size = u32_at(d, 0x18)? as usize;
    let aset = d.get(size..)?;
    if aset.get(..4) != Some(b"ASET") {
        return None;
    }
    let off = u64_at(aset, 0x18)? as usize;
    let len = u64_at(aset, 0x20)? as usize;
    let nacp = aset.get(off..off.checked_add(len)?)?;
    (0..16)
        .filter_map(|i| nacp.get(i * 0x300..i * 0x300 + 0x200))
        .map(|raw| {
            let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
            String::from_utf8_lossy(&raw[..end]).trim().to_string()
        })
        .find(|n| !n.is_empty())
}

/// Same rules as switchrecomp.py: npdm name unless it's the generic
/// "Application", else the folder/file name; then keep only safe characters.
pub fn default_title(npdm_name: Option<&str>, fallback: &str) -> String {
    let base = npdm_name.filter(|n| !n.is_empty() && *n != "Application").unwrap_or(fallback);
    sanitize_title(base)
}

pub fn sanitize_title(s: &str) -> String {
    let t: String = s.chars().filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | ' ')).collect();
    let t = t.trim();
    if t.is_empty() { "Game".into() } else { t.to_string() }
}

fn magic(p: &Path, off: u64) -> Option<[u8; 4]> {
    let mut f = File::open(p).ok()?;
    let mut buf = vec![0u8; off as usize + 4];
    f.read_exact(&mut buf).ok()?;
    buf[off as usize..].try_into().ok()
}

fn file_size(p: &Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

fn cstr_latin1(b: &[u8]) -> String {
    b.iter().take_while(|&&c| c != 0).map(|&c| c as char).collect()
}

fn u32_at(d: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(off..off + 4)?.try_into().ok()?))
}

fn u64_at(d: &[u8], off: usize) -> Option<u64> {
    Some(u64::from_le_bytes(d.get(off..off + 8)?.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_match_the_python_rules() {
        assert_eq!(default_title(Some("Application"), "My.Game v1.0"), "MyGame v10");
        assert_eq!(default_title(Some("Zelda"), "folder"), "Zelda");
        assert_eq!(default_title(None, "  "), "Game");
        assert_eq!(sanitize_title(" Space Game "), "Space Game");
    }
}
