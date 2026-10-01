use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Result;

pub(super) fn remote_join(root: &str, rel: &Path) -> String {
    let mut root = root.to_string();
    if !root.starts_with('/') {
        root.insert(0, '/');
    }
    root = root.trim_end_matches('/').to_string();
    let mut path = root;
    for part in rel
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .filter(|p| !p.is_empty() && p != ".")
    {
        path.push('/');
        path.push_str(&part);
    }
    if path.is_empty() {
        "/".to_string()
    } else {
        path
    }
}

pub(super) fn walkdir_sorted(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();

    fn walk(path: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
        out.push(path.to_path_buf());
        if path.is_dir() {
            let mut entries = fs::read_dir(path)?
                .flatten()
                .map(|e| e.path())
                .collect::<Vec<PathBuf>>();
            entries.sort();
            for entry in entries {
                walk(&entry, out)?;
            }
        }
        Ok(())
    }

    walk(root, &mut out)?;
    Ok(out)
}
