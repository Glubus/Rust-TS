//! Multi-file project compilation and dependency resolution.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::CompilerService;
use super::imports::extract_static_import_requests;
use super::resolver::ModuleResolver;
use crate::error::VmError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CompiledModule {
    pub(crate) module_id: String,
    pub(crate) transpiled_js: String,
    pub(crate) resolved_requests: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProjectCompileOutput {
    pub(crate) cache_seed: String,
    pub(crate) entry_module_id: String,
    pub(crate) modules: Vec<CompiledModule>,
}

pub(crate) fn compile_project(
    compiler: &mut CompilerService,
    entry_path: &Path,
) -> Result<ProjectCompileOutput, VmError> {
    let entry_path = normalize_entry_path(entry_path)?;
    let resolver = ModuleResolver::for_entry(&entry_path)?;
    let mut visited = BTreeMap::<PathBuf, CompiledModule>::new();
    let mut cache_parts = Vec::<(String, String)>::new();
    compile_module_recursive(
        compiler,
        &resolver,
        &entry_path,
        &mut visited,
        &mut cache_parts,
    )?;

    let entry_module_id = module_id(&entry_path)?;
    append_cache_metadata(&resolver, visited.keys(), &mut cache_parts)?;
    let cache_seed = build_cache_seed(&cache_parts);
    let modules = visited.into_values().collect();

    Ok(ProjectCompileOutput {
        cache_seed,
        entry_module_id,
        modules,
    })
}

fn compile_module_recursive(
    compiler: &mut CompilerService,
    resolver: &ModuleResolver,
    path: &Path,
    visited: &mut BTreeMap<PathBuf, CompiledModule>,
    cache_parts: &mut Vec<(String, String)>,
) -> Result<(), VmError> {
    if visited.contains_key(path) {
        return Ok(());
    }

    let source_text = fs::read_to_string(path)?;
    cache_parts.push((module_id(path)?, source_text.clone()));

    let requests = extract_static_import_requests(&source_text, path)?;
    let transpiled_js = compiler.execute_module(&source_text, path)?;
    let mut resolved_requests = BTreeMap::<String, String>::new();
    let mut dependencies = BTreeSet::<PathBuf>::new();

    for request in requests {
        let resolved_path = resolver.resolve_request(path, &request)?;
        resolved_requests.insert(request, module_id(&resolved_path)?);
        dependencies.insert(resolved_path);
    }

    visited.insert(
        path.to_path_buf(),
        CompiledModule {
            module_id: module_id(path)?,
            transpiled_js,
            resolved_requests,
        },
    );

    for dependency in dependencies {
        compile_module_recursive(compiler, resolver, &dependency, visited, cache_parts)?;
    }

    Ok(())
}

fn append_cache_metadata<'a>(
    resolver: &ModuleResolver,
    module_paths: impl Iterator<Item = &'a PathBuf>,
    cache_parts: &mut Vec<(String, String)>,
) -> Result<(), VmError> {
    for metadata_path in cache_metadata_paths(resolver.project_root(), module_paths)? {
        cache_parts.push((
            module_id(&metadata_path)?,
            fs::read_to_string(metadata_path)?,
        ));
    }
    Ok(())
}

fn cache_metadata_paths<'a>(
    project_root: &Path,
    module_paths: impl Iterator<Item = &'a PathBuf>,
) -> Result<BTreeSet<PathBuf>, VmError> {
    let mut paths = project_lockfiles(project_root)?;

    for module_path in module_paths {
        paths.extend(package_manifests_for_module(project_root, module_path)?);
    }

    Ok(paths)
}

fn project_lockfiles(project_root: &Path) -> Result<BTreeSet<PathBuf>, VmError> {
    const LOCKFILES: &[&str] = &[
        "package-lock.json",
        "npm-shrinkwrap.json",
        "pnpm-lock.yaml",
        "yarn.lock",
        "bun.lock",
    ];

    LOCKFILES
        .iter()
        .map(|name| project_root.join(name))
        .filter(|path| path.is_file())
        .map(canonicalize_cache_metadata_path)
        .collect()
}

fn package_manifests_for_module(
    project_root: &Path,
    module_path: &Path,
) -> Result<BTreeSet<PathBuf>, VmError> {
    let mut paths = BTreeSet::new();
    let mut current = module_path.parent();

    while let Some(dir) = current {
        if !dir.starts_with(project_root) {
            break;
        }

        let manifest = dir.join("package.json");
        if manifest.is_file() {
            paths.insert(canonicalize_cache_metadata_path(manifest)?);
        }

        if dir == project_root {
            break;
        }
        current = dir.parent();
    }

    Ok(paths)
}

fn canonicalize_cache_metadata_path(path: PathBuf) -> Result<PathBuf, VmError> {
    path.canonicalize().map_err(VmError::from)
}

fn normalize_entry_path(entry_path: &Path) -> Result<PathBuf, VmError> {
    if !entry_path.exists() {
        return Err(VmError::Resolve {
            details: format!("entry path does not exist: {}", entry_path.display()),
        });
    }

    entry_path.canonicalize().map_err(VmError::from)
}

fn module_id(path: &Path) -> Result<String, VmError> {
    path.canonicalize()
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(VmError::from)
}

fn build_cache_seed(parts: &[(String, String)]) -> String {
    let mut output = String::new();
    for (module_id, source) in parts {
        output.push_str(module_id);
        output.push('\n');
        output.push_str(source);
        output.push_str("\n---\n");
    }
    output
}
