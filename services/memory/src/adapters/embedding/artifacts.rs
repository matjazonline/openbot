//! The service's own model manifest (`assets/model-manifest.json`, D1): serving validates the
//! prepared local files (size and SHA-256, no symlinks) and never fetches anything. Adapted
//! from OpenBot's artifact validation; no OpenBot code or configuration is used.
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
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/model-manifest.json"
    )))
    .context("the bundled model manifest is invalid")
}

fn platform() -> anyhow::Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("osx-arm64"),
        ("linux", "aarch64") => Ok("linux-aarch64"),
        ("linux", "x86_64") => Ok("linux-x64"),
        _ => bail!("no pinned ONNX Runtime for this platform"),
    }
}

pub(super) struct Checked {
    pub model_id: String,
    pub runtime: PathBuf,
    /// The model files' bytes, read once and verified in memory: what was hashed is what loads.
    pub files: std::collections::HashMap<String, Vec<u8>>,
}

/// Every model file, bundled notice and this platform's runtime file must match the manifest.
pub(super) fn validate(directory: &Path) -> anyhow::Result<Checked> {
    let manifest = manifest()?;
    if !manifest.model_id.ends_with("/fp32/mean/384") {
        bail!("the model manifest does not name a 384-d mean-pooled fp32 model");
    }
    let platform = platform()?;
    let runtime = manifest
        .runtimes
        .iter()
        .find(|runtime| runtime.platform == platform)
        .context("no pinned ONNX Runtime for this platform")?;
    let mut files = std::collections::HashMap::new();
    for artifact in &manifest.files {
        files.insert(artifact.path.clone(), read_verified(directory, artifact)?);
    }
    // Notices are checked in place; the runtime library must be loaded from its path, so its
    // check-then-load window remains (the image's model directory is read-only).
    for artifact in manifest.bundled_files.iter().chain(&runtime.files) {
        check(directory, artifact)?;
    }
    Ok(Checked {
        model_id: manifest.model_id,
        runtime: directory.join(&runtime.library).canonicalize()?,
        files,
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Read one artifact whole (no symlinks) and verify its size and SHA-256 in memory.
pub(super) fn read_verified(directory: &Path, artifact: &Artifact) -> anyhow::Result<Vec<u8>> {
    let path = directory.join(&artifact.path);
    let context = || {
        format!(
            "model artifact missing or invalid at {}; run scripts/prepare-model.py --download \
             (build network only) and point MEMORY_MODEL_DIR at the prepared directory",
            path.display()
        )
    };
    let metadata = std::fs::symlink_metadata(&path).with_context(context)?;
    if !metadata.is_file() || metadata.len() != artifact.size {
        bail!("{}", context());
    }
    let mut bytes = Vec::with_capacity(artifact.size as usize);
    File::open(&path)
        .with_context(context)?
        .take(artifact.size + 1)
        .read_to_end(&mut bytes)
        .with_context(context)?;
    if bytes.len() as u64 != artifact.size || hex(&Sha256::digest(&bytes)) != artifact.sha256 {
        bail!("{}", context());
    }
    Ok(bytes)
}

pub(super) fn check(directory: &Path, artifact: &Artifact) -> anyhow::Result<()> {
    let path = directory.join(&artifact.path);
    let context = || {
        format!(
            "model artifact missing or invalid at {}; run scripts/prepare-model.py --download \
             (build network only) and point MEMORY_MODEL_DIR at the prepared directory",
            path.display()
        )
    };
    let metadata = std::fs::symlink_metadata(&path).with_context(context)?;
    if !metadata.is_file() || metadata.len() != artifact.size {
        bail!("{}", context());
    }
    let mut file = File::open(&path).with_context(context)?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
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
    let digest: String = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if read != artifact.size || digest != artifact.sha256 {
        bail!("{}", context());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_startup_model_artifacts_fail_on_missing_modified_resized_or_symlinked_files() {
        let dir = std::env::temp_dir().join(format!("memory-artifacts-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let digest: String = Sha256::digest(b"test")
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let expected = Artifact {
            path: "model.onnx".into(),
            size: 4,
            sha256: digest,
        };
        let missing = check(&dir, &expected).unwrap_err().to_string();
        assert!(missing.contains("prepare-model.py"), "{missing}");
        std::fs::write(dir.join("model.onnx"), b"test").unwrap();
        check(&dir, &expected).unwrap();
        assert_eq!(read_verified(&dir, &expected).unwrap(), b"test");
        for bytes in [b"xxxx".as_slice(), b"large", b"x"] {
            std::fs::write(dir.join("model.onnx"), bytes).unwrap();
            assert!(check(&dir, &expected).is_err());
            assert!(read_verified(&dir, &expected).is_err());
        }
        #[cfg(unix)]
        {
            std::fs::write(dir.join("source"), b"test").unwrap();
            std::fs::remove_file(dir.join("model.onnx")).unwrap();
            std::os::unix::fs::symlink(dir.join("source"), dir.join("model.onnx")).unwrap();
            assert!(check(&dir, &expected).is_err());
            assert!(read_verified(&dir, &expected).is_err());
        }
        std::fs::remove_dir_all(dir).unwrap();
        // The bundled manifest names the pinned model and is well formed.
        let manifest = manifest().unwrap();
        assert_eq!(
            manifest.model_id,
            "intfloat/multilingual-e5-small@614241f622f53c4eeff9890bdc4f31cfecc418b3/fp32/mean/384"
        );
    }
}
