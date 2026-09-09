//! Zero-dependency resource embedding for the Windows exe icon.
//!
//! Locates rc.exe from the Windows SDK (installed on this machine) and
//! compiles a tiny .rc that references assets/icon.ico into a .res file,
//! which is then passed to the MSVC linker. If rc.exe cannot be found the
//! build still succeeds — the program simply ends up without an embedded
//! icon.

use std::path::{Path, PathBuf};
use std::process::Command;

fn find_rc() -> Option<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for var in ["WindowsSdkDir", "WindowsSdkBinPath"] {
        if let Ok(v) = std::env::var(var) {
            if !v.is_empty() {
                roots.push(PathBuf::from(v));
            }
        }
    }
    roots.push(PathBuf::from(r"C:\Program Files (x86)\Windows Kits\10\bin"));
    roots.push(PathBuf::from(r"C:\Program Files\Windows Kits\10\bin"));

    for root in &roots {
        // Windows 10 SDK layout: bin\<version>\x64\rc.exe
        if let Ok(entries) = std::fs::read_dir(root) {
            let mut versions: Vec<PathBuf> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect();
            versions.sort();
            // try newest version dir first
            for dir in versions.iter().rev() {
                for arch in ["x64", "x86"] {
                    let cand = dir.join(arch).join("rc.exe");
                    if cand.is_file() {
                        return Some(cand);
                    }
                }
            }
            // maybe SDK layout is bin\x64\rc.exe directly
            for arch in ["x64", "x86"] {
                let cand = root.join(arch).join("rc.exe");
                if cand.is_file() {
                    return Some(cand);
                }
            }
        }
        // plain rc.exe on PATH
        if let Ok(p) = std::env::var("PATH") {
            for dir in std::env::split_paths(&p) {
                let cand = dir.join("rc.exe");
                if cand.is_file() {
                    return Some(cand);
                }
            }
        }
    }
    None
}

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");

    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let out_dir = std::env::var("OUT_DIR").unwrap_or_default();

    // .rc referencing the icon; rc.exe accepts forward slashes.
    let ico_path = Path::new(&manifest).join("assets").join("icon.ico");
    let ico_disp = ico_path.display().to_string().replace('\\', "/");
    let rc_content = format!("1 ICON \"{ico_disp}\"\n");

    let rc_path = Path::new(&out_dir).join("app.rc");
    if std::fs::write(&rc_path, rc_content).is_err() {
        println!("cargo:warning=dsb-launcher: could not write {rc_path:?}");
        return;
    }

    let Some(rc_exe) = find_rc() else {
        println!("cargo:warning=dsh-launcher: rc.exe not found; exe will have no embedded icon");
        return;
    };

    let res_path = Path::new(&out_dir).join("app.res");
    let status = Command::new(&rc_exe)
        .arg("/fo")
        .arg(&res_path)
        .arg(&rc_path)
        .status();

    match status {
        Ok(s) if s.success() => {
            // Ask rustc to hand the .res to the MSVC linker, which merges it
            // into the final exe resources.
            println!("cargo:rustc-link-arg-bins={}", res_path.display());
        }
        Ok(s) => println!("cargo:warning=dsh-launcher: rc.exe failed with {s}"),
        Err(e) => println!("cargo:warning=dsh-launcher: could not run rc.exe: {e}"),
    }
}
