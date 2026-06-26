//! Disk-backed cache store for transpiled JavaScript.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::identity::CacheIdentity;
use crate::error::VmError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CachedArtifact {
    pub(crate) cache_key: String,
    pub(crate) js_path: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) struct ScriptCache {
    root: PathBuf,
}

impl ScriptCache {
    pub(crate) fn new(root: impl Into<PathBuf>) -> Result<Self, VmError> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub(crate) fn cache_key(&self, identity: &CacheIdentity<'_>) -> Result<String, VmError> {
        let identity = serde_json::to_vec(identity)?;
        Ok(blake3::hash(&identity).to_hex().to_string())
    }

    pub(crate) fn load(&self, cache_key: &str) -> Result<Option<String>, VmError> {
        let path = self.js_path(cache_key);
        match fs::read_to_string(path) {
            Ok(source) => Ok(Some(source)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(VmError::Io(error)),
        }
    }

    pub(crate) fn store(&self, cache_key: &str, js: &str) -> Result<CachedArtifact, VmError> {
        let path = self.js_path(cache_key);
        fs::write(&path, js)?;
        Ok(CachedArtifact {
            cache_key: cache_key.to_owned(),
            js_path: path,
        })
    }

    pub(crate) fn entry_count(&self) -> Result<usize, VmError> {
        let mut count = 0usize;
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("js") {
                count += 1;
            }
        }
        Ok(count)
    }

    pub(crate) fn artifact_path(&self, cache_key: &str) -> PathBuf {
        self.js_path(cache_key)
    }

    fn js_path(&self, cache_key: &str) -> PathBuf {
        self.root.join(cache_file_name(cache_key))
    }
}

fn cache_file_name(cache_key: &str) -> String {
    let mut file_name = String::with_capacity(cache_key.len() + 3);
    file_name.push_str(cache_key);
    file_name.push_str(".js");
    file_name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_key_changes_when_host_abi_changes() {
        let cache = ScriptCache::new(std::env::temp_dir()).unwrap();
        let source = "export function run() { return 1; }";
        let first = CacheIdentity::inline(source, "abi-a");
        let second = CacheIdentity::inline(source, "abi-b");

        let first_key = cache.cache_key(&first).unwrap();
        let second_key = cache.cache_key(&second).unwrap();

        assert_ne!(first_key, second_key);
    }
}
