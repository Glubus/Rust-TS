//! TypeScript transpilation for the engine, with an optional disk cache.

use std::collections::BTreeSet;
use std::path::Path;

use crate::cache::{CacheIdentity, ScriptCache};
use crate::compiler::{CompilerService, ProjectCompileOutput, discover_project};
use crate::error::VmError;

/// Transpiles inline scripts and projects, reusing artifacts from the disk cache when
/// one is configured.
pub(crate) struct Transpiler {
    compiler: CompilerService,
    cache: Option<ScriptCache>,
}

impl Transpiler {
    pub(crate) fn new(cache_dir: Option<&Path>) -> Result<Self, VmError> {
        Ok(Self {
            compiler: CompilerService::default(),
            cache: cache_dir.map(ScriptCache::new).transpose()?,
        })
    }

    /// Transpiles one inline script. `host_abi` is part of the cache key.
    pub(crate) fn inline(
        &mut self,
        id: &str,
        source: &str,
        host_abi: &str,
    ) -> Result<String, VmError> {
        let cache_key = self.cache_key(&CacheIdentity::inline(source, host_abi))?;
        if let Some(cached) = self.load(cache_key.as_deref())? {
            return Ok(cached);
        }
        let source_path = Path::new(id).with_extension("ts");
        let transpiled = self.compiler.compile_inline(source, &source_path)?;
        self.store(cache_key.as_deref(), &transpiled)?;
        Ok(transpiled)
    }

    /// Resolves the project graph from `entry_path` and transpiles every module.
    /// Imports of `external_modules` stay unresolved; the module loader provides them.
    pub(crate) fn project(
        &mut self,
        entry_path: &Path,
        external_modules: &BTreeSet<String>,
        host_abi: &str,
    ) -> Result<ProjectCompileOutput, VmError> {
        let project = discover_project(entry_path, external_modules)?;
        let cache_key = self.cache_key(&CacheIdentity::project(&project.cache_seed, host_abi))?;
        // An artifact written by another version fails to parse and is rebuilt.
        if let Some(cached) = self.load(cache_key.as_deref())?
            && let Ok(output) = serde_json::from_str(&cached)
        {
            return Ok(output);
        }
        let output = self.compiler.transpile_project(project)?;
        self.store(cache_key.as_deref(), &serde_json::to_string(&output)?)?;
        Ok(output)
    }

    fn cache_key(&self, identity: &CacheIdentity<'_>) -> Result<Option<String>, VmError> {
        self.cache
            .as_ref()
            .map(|cache| cache.cache_key(identity))
            .transpose()
    }

    fn load(&self, cache_key: Option<&str>) -> Result<Option<String>, VmError> {
        match (&self.cache, cache_key) {
            (Some(cache), Some(key)) => cache.load(key),
            _ => Ok(None),
        }
    }

    fn store(&self, cache_key: Option<&str>, artifact: &str) -> Result<(), VmError> {
        if let (Some(cache), Some(key)) = (&self.cache, cache_key) {
            cache.store(key, artifact)?;
        }
        Ok(())
    }
}
