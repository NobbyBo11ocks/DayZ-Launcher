use std::path::PathBuf;
use std::{env, fs};

fn main() {
    copy_steam_dll();
    tauri_build::build()
}

/// `steam_api64.dll` (Steamworks SDK redistributable, vendored in `resources/`, D-040)
/// must sit next to any executable that links steamworks-sys: the dev binary in
/// `target/<profile>/` and test binaries in `target/<profile>/deps/`. The installer
/// gets it through `bundle.resources` in tauri.conf.json.
fn copy_steam_dll() {
    println!("cargo:rerun-if-changed=resources/steam_api64.dll");
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let src = manifest.join("resources").join("steam_api64.dll");
    if !src.is_file() {
        println!("cargo:warning=resources/steam_api64.dll missing; Steam features will fail to load at runtime");
        return;
    }
    // OUT_DIR = <target>/<profile>/build/<pkg>-<hash>/out
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let Some(profile_dir) = out.ancestors().nth(3) else {
        return;
    };
    for dir in [profile_dir.to_path_buf(), profile_dir.join("deps")] {
        let _ = fs::create_dir_all(&dir);
        let dst = dir.join("steam_api64.dll");
        let stale = match (fs::metadata(&src), fs::metadata(&dst)) {
            (Ok(a), Ok(b)) => a.len() != b.len(),
            _ => true,
        };
        if stale {
            let _ = fs::copy(&src, &dst);
        }
    }
}
