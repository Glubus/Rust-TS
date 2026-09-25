//! Disk-backed cache store for transpiled JavaScript.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use crate::error::VmError;

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

    pub(crate) fn load(&self, cache_key: &str) -> Result<Option<String>, VmError> {
        let path = self.js_path(cache_key);
        match fs::read_to_string(path) {
            Ok(source) => Ok(verified_payload(&source).map(str::to_owned)),
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData => Ok(None),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(VmError::Io(error)),
        }
    }

    pub(crate) fn store(&self, cache_key: &str, js: &str) -> Result<(), VmError> {
        let mut temporary = tempfile::NamedTempFile::new_in(&self.root)?;
        writeln!(
            temporary,
            "// rustts-cache-v2 {}",
            blake3::hash(js.as_bytes()).to_hex()
        )?;
        temporary.write_all(js.as_bytes())?;
        temporary.as_file().sync_all()?;
        persist_atomic(temporary, &self.js_path(cache_key))?;
        Ok(())
    }

    fn js_path(&self, cache_key: &str) -> PathBuf {
        self.root.join(cache_file_name(cache_key))
    }
}

fn verified_payload(source: &str) -> Option<&str> {
    let (header, payload) = source.split_once('\n')?;
    let expected = header.strip_prefix("// rustts-cache-v2 ")?;
    (blake3::hash(payload.as_bytes()).to_hex().as_str() == expected).then_some(payload)
}

fn persist_atomic(
    mut temporary: tempfile::NamedTempFile,
    path: &std::path::Path,
) -> std::io::Result<()> {
    // Windows can transiently deny replacement while another writer/AV has the
    // destination open. Retry the atomic rename; never delete the destination.
    for attempt in 0..50 {
        match temporary.persist(path) {
            Ok(_) => return Ok(()),
            Err(error)
                if cfg!(windows)
                    && error.error.kind() == std::io::ErrorKind::PermissionDenied
                    && attempt < 49 =>
            {
                temporary = error.file;
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            Err(error) => return Err(error.error),
        }
    }
    unreachable!("final attempt returns its error")
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
    fn concurrent_readers_only_see_complete_artifacts() {
        let root = tempfile::tempdir().unwrap();
        let cache = ScriptCache::new(root.path()).unwrap();
        let first = "a".repeat(64 * 1024);
        let second = "b".repeat(64 * 1024);
        cache.store("shared", &first).unwrap();
        std::thread::scope(|scope| {
            for source in [&first, &second] {
                let cache = &cache;
                scope.spawn(move || {
                    for _ in 0..30 {
                        cache.store("shared", source).unwrap();
                    }
                });
            }
            for _ in 0..100 {
                let loaded = cache.load("shared").unwrap().expect("complete artifact");
                assert!(loaded == first || loaded == second);
            }
        });
    }

    #[test]
    fn corrupt_and_legacy_entries_are_cache_misses() {
        let root = tempfile::tempdir().unwrap();
        let cache = ScriptCache::new(root.path()).unwrap();
        for content in ["legacy", "// rustts-cache-v2 invalid\npartial"] {
            fs::write(cache.js_path("broken"), content).unwrap();
            assert_eq!(cache.load("broken").unwrap(), None);
        }
        cache.store("broken", "export const answer = 42;").unwrap();
        assert_eq!(
            cache.load("broken").unwrap().unwrap(),
            "export const answer = 42;"
        );
    }
}
