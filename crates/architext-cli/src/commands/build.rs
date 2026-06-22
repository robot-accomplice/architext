//! `build [path] [--out <dir>]` — port of `buildStatic` in
//! `src/adapters/cli/architext-cli.mjs` (~line 1080).

use std::path::Path;
use std::process;

fn viewer_dist_dir() -> std::path::PathBuf {
    if let Ok(d) = std::env::var("ARCHITEXT_VIEWER_DIST") {
        return std::path::PathBuf::from(d);
    }
    std::path::PathBuf::from("viewer").join("dist")
}

pub fn run(target: &Path, out: &str) {
    let out_dir = if out.is_empty() {
        target.join("docs").join("architext").join("dist")
    } else {
        // JS: path.resolve(target, options.out)
        // If out is absolute use it as-is; otherwise resolve relative to target.
        let out_path = std::path::Path::new(out);
        if out_path.is_absolute() {
            out_path.to_path_buf()
        } else {
            target.join(out_path)
        }
    };

    let viewer_dist = viewer_dist_dir();
    if !viewer_dist.join("index.html").exists() {
        eprintln!("Package viewer assets are missing. Build the viewer first: trunk build --release --config crates/architext-viewer/Trunk.toml");
        process::exit(1);
    }

    let data_dir = target.join("docs").join("architext").join("data");

    // JS: await rm(outDir, { recursive: true, force: true })
    if out_dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(&out_dir) {
            eprintln!("Failed to remove {}: {e}", out_dir.display());
            process::exit(1);
        }
    }

    // JS: await cp(viewerDistDir, outDir, { recursive: true })
    if let Err(e) = copy_dir_all(&viewer_dist, &out_dir) {
        eprintln!("Failed to copy viewer dist: {e}");
        process::exit(1);
    }

    // JS: await mkdir(path.join(outDir, "data"), { recursive: true })
    let out_data_dir = out_dir.join("data");
    if let Err(e) = std::fs::create_dir_all(&out_data_dir) {
        eprintln!("Failed to create {}: {e}", out_data_dir.display());
        process::exit(1);
    }

    // JS: await cp(dataDir(target), path.join(outDir, "data"), { recursive: true })
    if let Err(e) = copy_dir_all(&data_dir, &out_data_dir) {
        eprintln!("Failed to copy data: {e}");
        process::exit(1);
    }

    // JS: console.log(`Copied target data to ${path.join(outDir, "data")}`)
    println!("Copied target data to {}", out_data_dir.display());
}

/// Recursively copy `src` into `dst` (not overwriting `dst` itself).
fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let target = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
