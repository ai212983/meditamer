use std::path::{Path, PathBuf};

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("tools dir")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

pub fn repo_path(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_root().join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_resolve_under_repo_root() {
        assert_eq!(
            repo_path("logs/evidence.log"),
            repo_root().join("logs/evidence.log")
        );
    }

    #[test]
    fn absolute_paths_are_preserved() {
        let absolute = PathBuf::from("/tmp/hostctl-evidence.log");
        assert_eq!(repo_path(&absolute), absolute);
    }
}
