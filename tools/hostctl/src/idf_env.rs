use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{anyhow, bail, Context, Result};

use crate::workflows::common::repo_root;

#[derive(Clone, Debug)]
pub struct IdfEnv {
    pub idf_root: PathBuf,
    pub python_bin: PathBuf,
    pub esptool_bin: PathBuf,
    pub idf_py_bin: Option<PathBuf>,
}

fn resolve_idf_root(explicit_root: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = explicit_root {
        return Ok(path.to_path_buf());
    }

    if let Ok(path) = std::env::var("IDF_APP_ROOT") {
        let path = PathBuf::from(path);
        if !path.as_os_str().is_empty() {
            return Ok(path);
        }
    }

    let search_root = repo_root().join(".embuild/espressif/esp-idf");
    let mut latest: Option<PathBuf> = None;
    for entry in std::fs::read_dir(&search_root)
        .with_context(|| format!("failed reading {}", search_root.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(OsStr::to_str) else {
            continue;
        };
        if !name.starts_with('v') {
            continue;
        }
        if latest
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(OsStr::to_str)
            .is_none_or(|current| name > current)
        {
            latest = Some(path);
        }
    }

    latest.ok_or_else(|| {
        anyhow!(
            "no local ESP-IDF install found under {} and IDF_APP_ROOT is not set",
            search_root.display()
        )
    })
}

pub fn bootstrap_idf_env(
    explicit_root: Option<&Path>,
    explicit_tools: Option<&Path>,
) -> Result<IdfEnv> {
    let idf_root = resolve_idf_root(explicit_root)?;
    let export_sh = idf_root.join("export.sh");
    if !export_sh.is_file() {
        bail!("ESP-IDF export.sh not found at {}", export_sh.display());
    }

    let mut command = Command::new("bash");
    command.arg("-c").arg(
        r#"set -euo pipefail
source "$IDF_EXPORT_SH" >/dev/null
printf 'PYTHON=%s\n' "$(command -v python)"
printf 'ESPTOOL=%s\n' "$(command -v esptool.py)"
if command -v idf.py >/dev/null 2>&1; then
  printf 'IDFPY=%s\n' "$(command -v idf.py)"
fi
"#,
    );
    command.env("IDF_EXPORT_SH", &export_sh);
    if let Some(path) = explicit_tools {
        command.env("IDF_TOOLS_PATH", path);
    } else if let Ok(path) = std::env::var("IDF_TOOLS_PATH") {
        if !path.trim().is_empty() {
            command.env("IDF_TOOLS_PATH", path);
        }
    }

    let output = command
        .output()
        .with_context(|| format!("failed to source {}", export_sh.display()))?;
    if !output.status.success() {
        bail!(
            "failed to source {}:\n{}",
            export_sh.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let mut python_bin = None;
    let mut esptool_bin = None;
    let mut idf_py_bin = None;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(value) = line.strip_prefix("PYTHON=") {
            python_bin = Some(PathBuf::from(value));
        } else if let Some(value) = line.strip_prefix("ESPTOOL=") {
            esptool_bin = Some(PathBuf::from(value));
        } else if let Some(value) = line.strip_prefix("IDFPY=") {
            idf_py_bin = Some(PathBuf::from(value));
        }
    }

    let python_bin = python_bin.ok_or_else(|| {
        anyhow!(
            "python was not available after sourcing {}",
            export_sh.display()
        )
    })?;
    let esptool_bin = esptool_bin.ok_or_else(|| {
        anyhow!(
            "esptool.py was not available after sourcing {}",
            export_sh.display()
        )
    })?;

    Ok(IdfEnv {
        idf_root,
        python_bin,
        esptool_bin,
        idf_py_bin,
    })
}

#[cfg(test)]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt};

    use tempfile::TempDir;

    use super::bootstrap_idf_env;

    fn write_executable(path: &Path, body: &str) {
        fs::write(path, body).expect("write script");
        let mut perms = fs::metadata(path).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).expect("chmod");
    }

    use std::path::Path;

    #[test]
    fn bootstrap_discovers_python_and_esptool_from_exported_env() {
        let temp = TempDir::new().expect("tempdir");
        let tools_bin = temp.path().join("fake-bin");
        fs::create_dir_all(&tools_bin).expect("fake-bin");

        let python = tools_bin.join("python");
        let esptool = tools_bin.join("esptool.py");
        let idf_py = tools_bin.join("idf.py");
        write_executable(&python, "#!/usr/bin/env bash\nexit 0\n");
        write_executable(&esptool, "#!/usr/bin/env bash\nexit 0\n");
        write_executable(&idf_py, "#!/usr/bin/env bash\nexit 0\n");

        let idf_root = temp.path().join("esp-idf/v5.3.4");
        fs::create_dir_all(&idf_root).expect("idf_root");
        let export_sh = idf_root.join("export.sh");
        fs::write(
            &export_sh,
            format!(
                "#!/usr/bin/env bash\nexport PATH=\"{}:$PATH\"\n",
                tools_bin.display()
            ),
        )
        .expect("export.sh");

        let env = bootstrap_idf_env(Some(&idf_root), None).expect("bootstraps");
        assert_eq!(env.idf_root, idf_root);
        assert_eq!(env.python_bin, python);
        assert_eq!(env.esptool_bin, esptool);
        assert_eq!(env.idf_py_bin.expect("idf.py"), idf_py);
    }
}
