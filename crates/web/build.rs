use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let dist = manifest.join("../../webapp/dist");
    let dist = dist.canonicalize().unwrap_or(dist);
    println!("cargo:rerun-if-changed={}", dist.display());

    let mut entries = Vec::new();
    collect(&dist, &dist, &mut entries);
    entries.sort();

    let mut source = String::from("pub static EMBEDDED: &[(&str, &[u8])] = &[\n");
    for (relative, absolute) in entries {
        let relative = relative.replace('\\', "/");
        let absolute = absolute.display().to_string().replace('\\', "/");
        source.push_str(&format!(
            "    ({:?}, include_bytes!({:?}) as &[u8]),\n",
            relative, absolute
        ));
    }
    source.push_str("];\n");

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("assets.rs");
    std::fs::write(out, source).expect("write assets.rs");
}

fn collect(base: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(base, &path, out);
        } else if path.is_file() {
            let relative = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            out.push((relative, path));
        }
    }
}
