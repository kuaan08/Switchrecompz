// SwitchRecomp - Copyright 2026 MZ Labs
// Licensed under the Apache License 2.0. See LICENSE.txt and NOTICE.txt;
// any copy or derived work must keep the credit to SwitchRecomp by MZ Labs.

//! Desktop front end for SwitchRecomp: picks a game, runs switchrecomp.py,
//! shows its progress and manages the recompiled games.
//!
//! `SwitchRecomp.exe --unpack [DIR]` unpacks the tools of the single-file
//! build (into DIR, or the usual cache folder) and exits: 0 on success,
//! 1 on failure, 2 when this build has nothing embedded.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod game;
mod icon;
mod ini;
mod job;
mod library;
mod payload;
mod settings;
mod theme;
mod toolchain;
mod updates;

use std::path::Path;
use std::sync::atomic::AtomicU64;

use eframe::egui;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--unpack") {
        std::process::exit(unpack_only(args.get(1).map(Path::new)));
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SwitchRecompz")
            .with_app_id("switchrecomp")
            .with_inner_size([1060.0, 720.0])
            .with_min_inner_size([780.0, 540.0])
            .with_icon(theme::app_icon())
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native("SwitchRecomp", options, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}

fn unpack_only(dir: Option<&Path>) -> i32 {
    if !payload::embedded() {
        eprintln!("this build has no embedded tools");
        return 2;
    }
    let done = AtomicU64::new(0);
    let result = match dir {
        Some(d) => payload::unpack_to(d, &done).map(|()| d.to_path_buf()),
        None => payload::unpack_cached(&done),
    };
    match result {
        Ok(d) => {
            eprintln!("unpacked to {}", d.display());
            0
        }
        Err(e) => {
            eprintln!("unpacking failed: {e}");
            1
        }
    }
}
