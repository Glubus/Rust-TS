//! TypeScript transpilation for the engine, per module, with an in-memory memo and an
//! optional disk cache.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::cache::{ScriptCache, module_cache_key};
use crate::compiler::{
    CompiledModule, CompilerService, ModuleOrigin, ProjectState, TranspiledModule, WatchedFiles,
    discover_project, extract_static_import_requests,
};
use crate::error::VmError;

/// Source type of inline scripts, which have no file extension.
const INLINE_SOURCE_TYPE: &str = "ts";

/// Transpiles inline scripts and project modules.
///
/// Each module is keyed by its content, so reloading a project only parses and
/// transpiles the files that changed: unchanged files come from the memo, or from the
/// disk cache after a restart. Each project also keeps its discovery state, so an
/// unchanged file is not even read while the project structure is unchanged.
pub(crate) struct Transpiler {
    compiler: CompilerService,
    cache: Option<ScriptCache>,
    memo: HashMap<String, ModuleMemo>,
    projects: HashMap<PathBuf, ProjectState>,
}

/// What one module source produced, filled lazily.
#[derive(Default)]
struct ModuleMemo {
    imports: Option<BTreeSet<String>>,
    transpiled: Option<TranspiledModule>,
}

/// One transpiled inline script.
pub(crate) struct TranspiledScript {
    pub(crate) js: String,
    pub(crate) module_origin: ModuleOrigin,
    pub(crate) module_key: String,
}

/// One transpiled project graph.
pub(crate) struct TranspiledProject {
    pub(crate) entry_module_id: String,
    pub(crate) modules: Vec<CompiledModule>,
    pub(crate) module_keys: Vec<String>,
    pub(crate) watched: WatchedFiles,
}

impl Transpiler {
    pub(crate) fn new(cache_dir: Option<&Path>) -> Result<Self, VmError> {
        Ok(Self {
            compiler: CompilerService::default(),
            cache: cache_dir.map(ScriptCache::new).transpose()?,
            memo: HashMap::new(),
            projects: HashMap::new(),
        })
    }

    /// Transpiles one inline script. Only successful compilations are remembered, so
    /// a rejected script (dynamic import, syntax error) fails on every load.
    pub(crate) fn inline(&mut self, id: &str, source: &str) -> Result<TranspiledScript, VmError> {
        let module_key = module_cache_key(source, INLINE_SOURCE_TYPE);
        let path = format!("{id}.{INLINE_SOURCE_TYPE}");
        let transpiled = self.transpiled(&module_key, |compiler| {
            compiler.compile_inline(source, Path::new(&path))
        })?;
        Ok(TranspiledScript {
            js: transpiled.js,
            module_origin: ModuleOrigin {
                path,
                source_map: transpiled.source_map,
            },
            module_key,
        })
    }

    /// Resolves the project graph from `entry_path` and transpiles every module.
    /// Imports of `external_modules` stay unresolved; the module loader provides them.
    pub(crate) fn project(
        &mut self,
        entry_path: &Path,
        external_modules: &BTreeSet<String>,
    ) -> Result<TranspiledProject, VmError> {
        let memo = &mut self.memo;
        let previous = self.projects.remove(entry_path);
        let (project, state) = discover_project(
            entry_path,
            external_modules,
            previous,
            &mut |source, path| {
                let entry = memo
                    .entry(module_cache_key(source, source_type(path)))
                    .or_default();
                if let Some(imports) = &entry.imports {
                    return Ok(imports.clone());
                }
                let imports = extract_static_import_requests(source, path)?;
                entry.imports = Some(imports.clone());
                Ok(imports)
            },
        )?;
        self.projects.insert(entry_path.to_path_buf(), state);

        let mut modules = Vec::with_capacity(project.modules.len());
        let mut module_keys = Vec::with_capacity(project.modules.len());
        for module in project.modules {
            let path = Path::new(&module.display_path);
            let module_key = module_cache_key(&module.source, source_type(path));
            let transpiled = self.transpiled(&module_key, |compiler| {
                compiler.compile_module(&module.source, path)
            })?;
            modules.push(CompiledModule {
                module_id: module.module_id,
                transpiled_js: transpiled.js,
                origin: ModuleOrigin {
                    path: module.display_path,
                    source_map: transpiled.source_map,
                },
                resolved_requests: module.resolved_requests,
            });
            module_keys.push(module_key);
        }

        Ok(TranspiledProject {
            entry_module_id: project.entry_module_id,
            modules,
            module_keys,
            watched: project.watched,
        })
    }

    /// Forgets memoized modules and project states no loaded script uses anymore.
    pub(crate) fn retain_used(&mut self, modules: &HashSet<&str>, projects: &HashSet<&Path>) {
        self.memo.retain(|key, _| modules.contains(key.as_str()));
        self.projects
            .retain(|entry_path, _| projects.contains(entry_path.as_path()));
    }

    /// The module transpiled from the memo, else from the disk cache, else by `compile`,
    /// whose result is remembered.
    fn transpiled(
        &mut self,
        module_key: &str,
        compile: impl FnOnce(&mut CompilerService) -> Result<TranspiledModule, VmError>,
    ) -> Result<TranspiledModule, VmError> {
        if let Some(transpiled) = self.remembered(module_key)? {
            return Ok(transpiled);
        }
        let transpiled = compile(&mut self.compiler)?;
        self.remember(module_key, &transpiled)?;
        Ok(transpiled)
    }

    /// Transpiled module from the memo, else from the disk cache.
    fn remembered(&mut self, module_key: &str) -> Result<Option<TranspiledModule>, VmError> {
        if let Some(transpiled) = self
            .memo
            .get(module_key)
            .and_then(|memo| memo.transpiled.clone())
        {
            return Ok(Some(transpiled));
        }
        let Some(transpiled) = self
            .cache
            .as_ref()
            .map(|cache| cache.load(module_key))
            .transpose()?
            .flatten()
        else {
            return Ok(None);
        };
        self.memo
            .entry(module_key.to_owned())
            .or_default()
            .transpiled = Some(transpiled.clone());
        Ok(Some(transpiled))
    }

    fn remember(&mut self, module_key: &str, transpiled: &TranspiledModule) -> Result<(), VmError> {
        if let Some(cache) = &self.cache {
            cache.store(module_key, transpiled)?;
        }
        self.memo
            .entry(module_key.to_owned())
            .or_default()
            .transpiled = Some(transpiled.clone());
        Ok(())
    }
}

fn source_type(path: &Path) -> &str {
    path.extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
}
