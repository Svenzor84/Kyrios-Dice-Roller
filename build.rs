use std::fs;
use std::io;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir  = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    let lib_dir  = manifest.join("lib");
    let zips_dir = manifest.join("sdl-zips");

    // OUT_DIR is target/{profile}/build/{crate}-{hash}/out — three levels up is target/{profile}
    let exe_dir = out_dir.ancestors().nth(3).unwrap().to_path_buf();

    fs::create_dir_all(&lib_dir).expect("could not create lib/");

    // Re-run only when the sdl-zips directory contents change
    println!("cargo:rerun-if-changed=sdl-zips/");

    if zips_dir.exists() {
        for entry in fs::read_dir(&zips_dir).unwrap().flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("zip") {
                extract_sdl_zip(&path, &lib_dir, &exe_dir);
            }
        }
    } else {
        println!("cargo:warning=sdl-zips/ not found — place SDL3 VC zip files there");
    }

    println!("cargo:rustc-link-search={}", lib_dir.display());

    // Copy assets/ from project root → target/{profile}/assets/ so the exe can
    // find them whether launched from an IDE, a terminal, or double-clicked.
    println!("cargo:rerun-if-changed=assets/");
    let src_assets = manifest.join("assets");
    let dst_assets = exe_dir.join("assets");
    if src_assets.exists() {
        copy_dir_all(&src_assets, &dst_assets).expect("failed to copy assets/");
    }
}

fn copy_dir_all(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)?.flatten() {
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_all(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn extract_sdl_zip(zip_path: &Path, lib_dir: &Path, exe_dir: &Path) {
    let file = match fs::File::open(zip_path) {
        Ok(f) => f,
        Err(e) => {
            println!("cargo:warning=Could not open {:?}: {}", zip_path.file_name().unwrap(), e);
            return;
        }
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(e) => {
            println!("cargo:warning=Could not read zip {:?}: {}", zip_path.file_name().unwrap(), e);
            return;
        }
    };

    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let name = entry.name().to_string();

        // Extract x64 .lib files → lib/
        if name.contains("/lib/x64/") && name.ends_with(".lib") {
            let filename = Path::new(&name).file_name().unwrap();
            let dest = lib_dir.join(filename);
            let mut out = fs::File::create(&dest).unwrap();
            io::copy(&mut entry, &mut out).unwrap();
            println!("cargo:warning=Extracted lib: {}", filename.to_string_lossy());
        }

        // Extract x64 .dll files → target/{profile}/ (next to the .exe)
        if name.contains("/lib/x64/") && name.ends_with(".dll") {
            fs::create_dir_all(exe_dir).ok();
            let filename = Path::new(&name).file_name().unwrap();
            let dest = exe_dir.join(filename);
            let mut out = fs::File::create(&dest).unwrap();
            io::copy(&mut entry, &mut out).unwrap();
            println!("cargo:warning=Extracted dll: {}", filename.to_string_lossy());
        }
    }
}
