//! Trusted build manifest; serving validates local files and never fetches artifacts.
use anyhow::{Context, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};
#[derive(Deserialize)]
pub(super) struct Manifest {
    pub model_id: String,
    pub files: Vec<Artifact>,
    pub bundled_files: Vec<Artifact>,
    pub runtimes: Vec<Runtime>,
}
#[derive(Deserialize)]
pub(super) struct Runtime {
    pub platform: String,
    pub library: String,
    pub files: Vec<Artifact>,
}
#[derive(Deserialize)]
pub(super) struct Artifact {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}
pub(super) fn manifest() -> anyhow::Result<Manifest> {
    serde_json::from_str(include_str!(
        "../../../assets/knowledge/model-manifest.json"
    ))
    .context("Invalid baked knowledge manifest")
}
fn platform() -> anyhow::Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("osx-arm64"),
        ("linux", "aarch64") => Ok("linux-aarch64"),
        ("linux", "x86_64") => Ok("linux-x64"),
        _ => bail!("No baked knowledge runtime for this platform"),
    }
}
pub(super) struct CheckedArtifacts {
    pub id: String,
    pub runtime: PathBuf,
}
pub(super) fn validate(directory: &Path) -> anyhow::Result<CheckedArtifacts> {
    let manifest = manifest()?;
    let platform = platform()?;
    let runtime = manifest
        .runtimes
        .iter()
        .find(|runtime| runtime.platform == platform)
        .context("No baked knowledge runtime for this platform")?;
    for artifact in manifest
        .files
        .iter()
        .chain(&manifest.bundled_files)
        .chain(&runtime.files)
    {
        check(directory, artifact)?;
    }
    Ok(CheckedArtifacts {
        id: manifest.model_id,
        runtime: directory.join(&runtime.library).canonicalize()?,
    })
}
pub(super) fn check(directory: &Path, artifact: &Artifact) -> anyhow::Result<()> {
    let path = directory.join(&artifact.path);
    let context = || {
        format!(
            "Knowledge artifact missing or invalid at {}; run scripts/prepare-knowledge-model.py",
            path.display()
        )
    };
    let metadata = std::fs::symlink_metadata(&path).with_context(context)?;
    if !metadata.is_file() || metadata.len() != artifact.size {
        bail!("{}", context());
    }
    let mut file = File::open(&path).with_context(context)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut read = 0u64;
    loop {
        let length = file.read(&mut buffer).with_context(context)?;
        if length == 0 {
            break;
        }
        read += length as u64;
        if read > artifact.size {
            bail!("{}", context());
        }
        hash.update(&buffer[..length]);
    }
    if read != artifact.size || format!("{:x}", hash.finalize()) != artifact.sha256 {
        bail!("{}", context());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn knowledge_model_artifacts_fail_on_missing_modified_wrong_size_or_symlink_files() {
        let dir =
            std::env::temp_dir().join(format!("knowledge-artifacts-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let expected = Artifact {
            path: "model.onnx".into(),
            size: 4,
            sha256: format!("{:x}", Sha256::digest(b"test")),
        };
        assert!(
            check(&dir, &expected)
                .unwrap_err()
                .to_string()
                .contains(dir.to_str().unwrap())
        );
        std::fs::write(dir.join("model.onnx"), b"test").unwrap();
        check(&dir, &expected).unwrap();
        for bytes in [b"xxxx".as_slice(), b"large", b"x"] {
            std::fs::write(dir.join("model.onnx"), bytes).unwrap();
            assert!(check(&dir, &expected).is_err());
        }
        #[cfg(unix)]
        {
            std::fs::write(dir.join("source"), b"test").unwrap();
            std::fs::remove_file(dir.join("model.onnx")).unwrap();
            std::os::unix::fs::symlink(dir.join("source"), dir.join("model.onnx")).unwrap();
            assert!(check(&dir, &expected).is_err());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
