//! Embeds the app icon and version info in SwitchRecomp.exe.
//!
//! With SWR_PAYLOAD_DIR set to a folder laid out like the release (scripts,
//! runtime\, bin\, python\, tcc\, w64devkit\), it also packs that folder into
//! the exe: the single-file build unpacks it on first start (src/payload.rs).
//!
//! SWR_VERSION (set by CI to the release tag or dev-<commit>) names the build in
//! the About tab and the exe's file properties.

#[path = "src/icon.rs"]
mod icon;

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=src/icon.rs");
    println!("cargo:rerun-if-env-changed=SWR_PAYLOAD_DIR");
    println!("cargo:rerun-if-env-changed=SWR_VERSION");
    println!("cargo::rustc-check-cfg=cfg(payload)");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let version = build_version();
    println!("cargo:rustc-env=SWR_BUILD={version}");
    if let Some(dir) = std::env::var_os("SWR_PAYLOAD_DIR").filter(|d| !d.is_empty()) {
        pack_payload(Path::new(&dir), &out);
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_icon(&out, &version);
    }
}

/// The CI version, or the crate version marked as a local build.
fn build_version() -> String {
    match std::env::var("SWR_VERSION") {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => format!("{} (local build)", std::env::var("CARGO_PKG_VERSION").unwrap_or_default()),
    }
}

/// tar + zstd of the payload folder, with sorted entries and fixed metadata so
/// the same folder always gives the same archive (and the same unpack folder).
fn pack_payload(dir: &Path, out: &Path) {
    assert!(dir.join("switchrecomp.py").is_file(), "SWR_PAYLOAD_DIR has no switchrecomp.py: {}", dir.display());
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut files = Vec::new();
    collect(dir, Path::new(""), &mut files);
    let archive = out.join("payload.tar.zst");
    let mut enc = zstd::stream::write::Encoder::new(BufWriter::new(File::create(&archive).unwrap()), 19).unwrap();
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get() as u32);
    enc.multithread(threads).unwrap();
    // long matches pay off on the many similar GCC libraries; unpacking needs a 128 MB window
    enc.long_distance_matching(true).unwrap();
    enc.window_log(27).unwrap();
    let mut tar = tar::Builder::new(enc);
    tar.mode(tar::HeaderMode::Deterministic);
    tar.follow_symlinks(false);
    for rel in &files {
        tar.append_path_with_name(dir.join(rel), rel).unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap().flush().unwrap();
    let id = fnv1a(&std::fs::read(&archive).unwrap());
    println!("cargo:rustc-cfg=payload");
    println!("cargo:rustc-env=SWR_PAYLOAD_FILE={}", archive.display());
    println!("cargo:rustc-env=SWR_PAYLOAD_ID={id:016x}");
}

/// Files under `dir`, as sorted relative paths; __pycache__ is left out.
fn collect(root: &Path, rel: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(root.join(rel)).unwrap().flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = rel.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            if e.file_name() != "__pycache__" {
                collect(root, &name, out);
            }
        } else {
            out.push(name);
        }
    }
}

fn fnv1a(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3))
}

fn embed_icon(out: &Path, version: &str) {
    let ico = out.join("app.ico");
    std::fs::write(&ico, ico_file(&[16, 24, 32, 48, 64, 128])).expect("write app.ico");
    let mut res = winresource::WindowsResource::new();
    res.set_icon(&ico.to_string_lossy())
        .set("FileDescription", "SwitchRecomp")
        .set("ProductName", "SwitchRecomp")
        .set("ProductVersion", version)
        .set("LegalCopyright", "Copyright 2026 MZ Labs");
    // a missing resource compiler only costs the icon, not the build
    if let Err(e) = res.compile() {
        println!("cargo:warning=exe icon not embedded: {e}");
    }
}

/// An .ico with one 32-bit BMP image per size.
fn ico_file(sizes: &[u32]) -> Vec<u8> {
    let images: Vec<Vec<u8>> = sizes.iter().map(|&n| bmp(n)).collect();
    let mut out = Vec::new();
    out.extend(0u16.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend((sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (&n, img) in sizes.iter().zip(&images) {
        let dim = if n >= 256 { 0 } else { n as u8 };
        out.extend([dim, dim, 0, 0]);
        out.extend(1u16.to_le_bytes());
        out.extend(32u16.to_le_bytes());
        out.extend((img.len() as u32).to_le_bytes());
        out.extend(offset.to_le_bytes());
        offset += img.len() as u32;
    }
    for img in images {
        out.extend(img);
    }
    out
}

/// BITMAPINFOHEADER + bottom-up BGRA pixels + an empty AND mask (alpha does the masking).
fn bmp(n: u32) -> Vec<u8> {
    let px = icon::rgba(n);
    let mask_row = n.div_ceil(32) * 4;
    let mut v = Vec::new();
    v.extend(40u32.to_le_bytes());
    v.extend((n as i32).to_le_bytes());
    v.extend((2 * n as i32).to_le_bytes());
    v.extend(1u16.to_le_bytes());
    v.extend(32u16.to_le_bytes());
    v.extend(0u32.to_le_bytes());
    v.extend((n * n * 4 + mask_row * n).to_le_bytes());
    v.extend([0u8; 16]);
    for y in (0..n).rev() {
        for x in 0..n {
            let i = ((y * n + x) * 4) as usize;
            v.extend([px[i + 2], px[i + 1], px[i], px[i + 3]]);
        }
    }
    v.extend(vec![0u8; (mask_row * n) as usize]);
    v
}
