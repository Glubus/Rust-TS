//! Compile and cache preparation before worker mounting.

use std::path::Path;

use crate::cache::CacheIdentity;
use crate::compiler::{CompiledScript, ProjectCompileOutput, validate_static_module_graph};
use crate::error::VmError;

use super::script_manager::ScriptManager;

impl ScriptManager {
    pub(crate) fn prepare_script(
        &self,
        script_id: &str,
        source: &str,
    ) -> Result<CompiledScript, VmError> {
        let source_path = Path::new(script_id).with_extension("ts");
        validate_static_module_graph(source, &source_path)?;

        let host_abi = self.inner.host_contract_registry.cache_abi_seed()?;
        let cache_identity = CacheIdentity::inline(source, &host_abi);
        let cache_key = self.inner.cache.cache_key(&cache_identity)?;
        let transpiled_path = self.inner.cache.artifact_path(&cache_key);

        if let Some(transpiled_js) = self.inner.cache.load(&cache_key)? {
            return Ok(CompiledScript {
                cache_key,
                transpiled_js,
                transpiled_path,
                entry_path: None,
                modules: Vec::new(),
            });
        }

        let mut compiler = self
            .inner
            .compiler
            .lock()
            .map_err(|_| VmError::WorkerPanicked)?;
        let compiled = compiler.compile_script(cache_key, source, &source_path, transpiled_path)?;
        self.inner
            .cache
            .store(&compiled.cache_key, &compiled.transpiled_js)?;
        Ok(compiled)
    }

    pub(crate) fn prepare_project_script(
        &self,
        entry_path: &Path,
    ) -> Result<CompiledScript, VmError> {
        let mut compiler = self
            .inner
            .compiler
            .lock()
            .map_err(|_| VmError::WorkerPanicked)?;
        let project = compiler.compile_project(entry_path)?;
        let host_abi = self.inner.host_contract_registry.cache_abi_seed()?;
        let cache_identity = CacheIdentity::project(&project.cache_seed, &host_abi);
        let cache_key = self.inner.cache.cache_key(&cache_identity)?;
        let transpiled_path = self.inner.cache.artifact_path(&cache_key);

        if let Some(cached_project) = self.inner.cache.load(&cache_key)? {
            return self.cached_project_script(cache_key, transpiled_path, &cached_project);
        }

        let compiled = compiler.build_project_script(cache_key, transpiled_path, &project)?;
        let cached_project = serde_json::to_string(&project)?;
        self.inner
            .cache
            .store(&compiled.cache_key, &cached_project)?;
        Ok(compiled)
    }

    fn cached_project_script(
        &self,
        cache_key: String,
        transpiled_path: std::path::PathBuf,
        cached_project: &str,
    ) -> Result<CompiledScript, VmError> {
        let project: ProjectCompileOutput = serde_json::from_str(cached_project)?;
        Ok(CompiledScript {
            cache_key,
            transpiled_js: String::new(),
            transpiled_path,
            entry_path: Some(project.entry_module_id.clone()),
            modules: project.modules,
        })
    }
}
