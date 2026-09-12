//! Zero-dependency resource embedding for the Windows exe icon.
//!
//! Locates rc.exe from the Windows SDK and compiles a tiny .rc that references
//! assets/icon.ico into a .res file, which is then passed to the MSVC linker.
//! If rc.exe cannot be found the build still succeeds — the program simply ends
//! up without an embedded icon.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Candidate `rc.exe` locations, in order: SDK environment variables, the two
/// standard Windows Kits roots (both `bin\<version>\<arch>` and `bin\<arch>`),
/// then PATH. Newest SDK version wins.
fn find_rc() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    // Both of these point straight at the directory holding rc.exe.
    for var in ["RcExePath", "WindowsSdkBinPath"] {
        if let Ok(v) = std::env::var(var) {
            if !v.is_empty() {
                candidates.push(PathBuf::from(v).join("rc.exe"));
            }
        }
    }

    let mut sdk_bin_roots: Vec<PathBuf> = Vec::new();
    if let Ok(v) = std::env::var("WindowsSdkDir") {
        if !v.is_empty() {
            // WindowsSdkDir is the Kits root (…\Windows Kits\10), so rc lives
            // under its bin\<version>\<arch>.
            sdk_bin_roots.push(PathBuf::from(v).join("bin"));
        }
    }
    sdk_bin_roots.push(PathBuf::from(r"C:\Program Files (x86)\Windows Kits\10\bin"));
    sdk_bin_roots.push(PathBuf::from(r"C:\Program Files\Windows Kits\10\bin"));

    for root in &sdk_bin_roots {
        // bin\<version>\<arch>\rc.exe — newest version first.
        if let Ok(entries) = std::fs::read_dir(root) {
            let mut versions: Vec<PathBuf> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect();
            versions.sort();
            for dir in versions.iter().rev() {
                for arch in ["x64", "x86"] {
                    candidates.push(dir.join(arch).join("rc.exe"));
                    candidates.push(dir.join("rc.exe"));
                }
            }
        }
        // bin\<arch>\rc.exe
        for arch in ["x64", "x86"] {
            candidates.push(root.join(arch).join("rc.exe"));
        }
    }

    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            candidates.push(dir.join("rc.exe"));
        }
    }

    candidates.into_iter().find(|p| p.is_file())
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
        println!("cargo:warning=dsh-launcher: could not write {rc_path:?}");
        return;
    }

    let Some(rc_exe) = find_rc() else {
        println!("cargo:warning=dsh-launcher: rc.exe not found; exe will have no embedded icon");
        return;
    };

    let res_path = Path::new(&out_dir).join("app.res");
    // /c 65001 makes rc read the .rc as UTF-8, so a project path containing
    // non-ASCII characters still resolves (the default code page would mangle it
    // and silently drop the icon).
    let status = Command::new(&rc_exe)
        .arg("/nologo")
        .arg("/c")
        .arg("65001")
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
