//! Multi-file project discovery: the static ESM graph reachable from an entry file,
//! rebuilt incrementally from the previous discovery of the same project.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use super::resolver::ModuleResolver;
use super::source_map::ModuleOrigin;
use super::stamp::{FileStamp, WatchedFiles};
use crate::error::VmError;

/// Returns the static import requests of one module source. The path is the module's
/// display path: it names the module in diagnostics and gives its source type.
pub(crate) type ImportsOf<'a> = dyn FnMut(&str, &Path) -> Result<BTreeSet<String>, VmError> + 'a;

/// One transpiled module, ready for the module store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompiledModule {
    pub(crate) module_id: String,
    pub(crate) transpiled_js: String,
    pub(crate) origin: ModuleOrigin,
    pub(crate) resolved_requests: BTreeMap<String, String>,
}

pub(crate) struct DiscoveredProject {
    pub(crate) entry_module_id: String,
    pub(crate) modules: Vec<DiscoveredModule>,
    /// Module files plus everything resolution depended on.
    pub(crate) watched: WatchedFiles,
}

pub(crate) struct DiscoveredModule {
    pub(crate) module_id: String,
    /// The path relative to the project root, with `/` separators.
    pub(crate) display_path: String,
    pub(crate) source: String,
    pub(crate) resolved_requests: BTreeMap<String, String>,
}

/// What the discovery of one project learned, reused by its next discovery.
///
/// Resolution only depends on the project structure (which files exist,
/// `tsconfig.json`, `package.json`), never on module contents. While the structure is
/// unchanged, the resolver keeps its file system cache and a module whose file is
/// unchanged is neither read, nor parsed, nor resolved again.
pub(crate) struct ProjectState {
    resolver: ModuleResolver,
    /// Directories from each module up to the project root, `tsconfig.json` and the
    /// `package.json` files resolution read.
    structure: WatchedFiles,
    /// Host module names imports were resolved against.
    external_modules: BTreeSet<String>,
    modules: HashMap<PathBuf, KnownModule>,
}

struct KnownModule {
    stamp: Option<FileStamp>,
    display_path: String,
    source: String,
    resolved_requests: BTreeMap<String, String>,
    dependencies: Vec<PathBuf>,
    package_jsons: Vec<PathBuf>,
}

/// Walks the static import graph from `entry_path`, reusing `previous` when it is the
/// state of the same project. `imports_of` returns the static import requests of one
/// module source; requests naming `external_modules` stay unresolved for the module
/// loader. Returns the graph and the state for the next discovery.
pub(crate) fn discover_project(
    entry_path: &Path,
    external_modules: &BTreeSet<String>,
    previous: Option<ProjectState>,
    imports_of: &mut ImportsOf<'_>,
) -> Result<(DiscoveredProject, ProjectState), VmError> {
    let entry_path = normalize_entry_path(entry_path)?;
    let (resolver, structure, known, structure_unchanged) = match previous {
        Some(state)
            if state.external_modules == *external_modules && !state.structure.changed() =>
        {
            (state.resolver, state.structure, state.modules, true)
        }
        Some(state) => (
            ModuleResolver::for_entry(&entry_path)?,
            WatchedFiles::default(),
            state.modules,
            false,
        ),
        None => (
            ModuleResolver::for_entry(&entry_path)?,
            WatchedFiles::default(),
            HashMap::new(),
            false,
        ),
    };
    let mut discovery = Discovery {
        resolver: &resolver,
        external_modules,
        imports_of,
        known,
        structure_unchanged,
        structure,
        modules: BTreeMap::new(),
    };
    if let Some(tsconfig) = resolver.tsconfig_path() {
        discovery.structure.watch(tsconfig);
    }
    discovery.visit(entry_path.clone())?;

    let Discovery {
        structure,
        modules: visited,
        ..
    } = discovery;
    let mut watched = structure.clone();
    let mut modules = Vec::with_capacity(visited.len());
    let mut known = HashMap::with_capacity(visited.len());
    for (path, module) in visited {
        watched.watch_stamped(&path, module.stamp);
        modules.push(DiscoveredModule {
            module_id: module_id(&path),
            display_path: module.display_path.clone(),
            source: module.source.clone(),
            resolved_requests: module.resolved_requests.clone(),
        });
        known.insert(path, module);
    }

    Ok((
        DiscoveredProject {
            entry_module_id: module_id(&entry_path),
            modules,
            watched,
        },
        ProjectState {
            resolver,
            structure,
            external_modules: external_modules.clone(),
            modules: known,
        },
    ))
}

struct Discovery<'a> {
    resolver: &'a ModuleResolver,
    external_modules: &'a BTreeSet<String>,
    imports_of: &'a mut ImportsOf<'a>,
    /// Modules of the previous discovery.
    known: HashMap<PathBuf, KnownModule>,
    /// Whether the previous resolutions still hold.
    structure_unchanged: bool,
    structure: WatchedFiles,
    modules: BTreeMap<PathBuf, KnownModule>,
}

impl Discovery<'_> {
    /// `path` is canonical: the entry is canonicalized once and resolution returns
    /// canonical paths, so it is also the module id.
    fn visit(&mut self, path: PathBuf) -> Result<(), VmError> {
        if self.modules.contains_key(&path) {
            return Ok(());
        }

        // A file added in any directory between the module and the project root can
        // take precedence in resolution (`./src/value` -> `src/value.ts` over
        // `src/value/index.ts`), so every such directory is watched.
        for directory in path.ancestors().skip(1) {
            if !directory.starts_with(self.resolver.project_root()) {
                break;
            }
            self.structure.watch(directory);
        }

        // Stamped before reading: an edit racing this read shows up as a change.
        let stamp = FileStamp::of(&path);
        let module = match self.known.remove(&path) {
            Some(known) if stamp.is_some() && known.stamp == stamp && self.structure_unchanged => {
                known
            }
            Some(known) if stamp.is_some() && known.stamp == stamp => {
                self.resolve_module(&path, stamp, known.source)?
            }
            _ => {
                let source = fs::read_to_string(&path)?;
                self.resolve_module(&path, stamp, source)?
            }
        };
        for package_json in &module.package_jsons {
            self.structure.watch(package_json);
        }
        let dependencies = module.dependencies.clone();
        self.modules.insert(path, module);

        for dependency in dependencies {
            self.visit(dependency)?;
        }
        Ok(())
    }

    fn resolve_module(
        &mut self,
        path: &Path,
        stamp: Option<FileStamp>,
        source: String,
    ) -> Result<KnownModule, VmError> {
        let display_path = display_path(self.resolver.project_root(), path);
        let requests = (self.imports_of)(&source, Path::new(&display_path))?;
        let mut resolved_requests = BTreeMap::new();
        let mut dependencies = BTreeSet::new();
        let mut package_jsons = Vec::new();

        for request in requests {
            if self.external_modules.contains(&request) {
                resolved_requests.insert(request.clone(), request);
                continue;
            }
            let resolved = self.resolver.resolve_request(path, &request)?;
            package_jsons.extend(resolved.package_json);
            resolved_requests.insert(request, module_id(&resolved.path));
            dependencies.insert(resolved.path);
        }

        Ok(KnownModule {
            stamp,
            display_path,
            source,
            resolved_requests,
            dependencies: dependencies.into_iter().collect(),
            package_jsons,
        })
    }
}

fn normalize_entry_path(entry_path: &Path) -> Result<PathBuf, VmError> {
    if !entry_path.exists() {
        return Err(VmError::Resolve {
            details: format!("entry path does not exist: {}", entry_path.display()),
        });
    }

    entry_path.canonicalize().map_err(VmError::from)
}

fn module_id(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// How diagnostics and stack traces name a module: its path relative to the project
/// root with `/` separators, else its full path.
fn display_path(project_root: &Path, path: &Path) -> String {
    let Ok(relative) = path.strip_prefix(project_root) else {
        return module_id(path);
    };
    relative
        .iter()
        .map(|part| part.to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}
