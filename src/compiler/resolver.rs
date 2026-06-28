//! Project module resolution backed by `oxc_resolver`.

use std::path::{Path, PathBuf};

use oxc_resolver::{
    ResolveOptions, Resolver, TsconfigDiscovery, TsconfigOptions, TsconfigReferences,
};

use crate::error::VmError;

const MODULE_EXTENSIONS: &[&str] = &[".ts", ".tsx", ".mts", ".js", ".jsx", ".mjs"];

#[derive(Debug)]
pub(crate) struct ModuleResolver {
    resolver: Resolver,
    project_root: PathBuf,
}

impl Default for ModuleResolver {
    fn default() -> Self {
        let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            resolver: Resolver::new(resolve_options(None)),
            project_root,
        }
    }
}

impl ModuleResolver {
    pub(crate) fn for_entry(entry_path: &Path) -> Result<Self, VmError> {
        let entry_dir = entry_path.parent().ok_or_else(|| VmError::Resolve {
            details: format!("entry has no parent directory: {}", entry_path.display()),
        })?;
        let tsconfig_path = find_tsconfig(entry_dir);
        let project_root = project_root(entry_dir, tsconfig_path.as_deref())?;
        let resolver = Resolver::new(resolve_options(tsconfig_path.as_deref()));

        Ok(Self {
            resolver,
            project_root,
        })
    }

    pub(crate) fn resolve_request(
        &self,
        from_path: &Path,
        request: &str,
    ) -> Result<PathBuf, VmError> {
        let base_dir = parent_dir(from_path)?;
        let resolved_path = self.resolve_from_dir(base_dir, request, from_path)?;
        self.validate_project_scope(request, &resolved_path)?;

        Ok(resolved_path)
    }

    pub(crate) fn project_root(&self) -> &Path {
        &self.project_root
    }

    fn resolve_from_dir(
        &self,
        base_dir: &Path,
        request: &str,
        from_path: &Path,
    ) -> Result<PathBuf, VmError> {
        self.resolver
            .resolve(base_dir, request)
            .map(|resolution| resolution.into_path_buf())
            .map_err(|error| VmError::Resolve {
                details: format!(
                    "unable to resolve import `{request}` from {}: {error}",
                    from_path.display()
                ),
            })
    }

    fn validate_project_scope(&self, request: &str, resolved_path: &Path) -> Result<(), VmError> {
        let canonical_resolved_path = resolved_path.canonicalize().map_err(VmError::from)?;
        if canonical_resolved_path.starts_with(&self.project_root) {
            return Ok(());
        }

        Err(VmError::Resolve {
            details: format!(
                "resolved import `{request}` outside project root {}: {}",
                self.project_root.display(),
                canonical_resolved_path.display()
            ),
        })
    }
}

fn resolve_options(tsconfig_path: Option<&Path>) -> ResolveOptions {
    ResolveOptions {
        tsconfig: tsconfig_path.map(|config_file| {
            TsconfigDiscovery::Manual(TsconfigOptions {
                config_file: config_file.to_path_buf(),
                references: TsconfigReferences::Disabled,
            })
        }),
        extensions: MODULE_EXTENSIONS
            .iter()
            .map(|extension| (*extension).to_owned())
            .collect(),
        main_files: vec![String::from("index")],
        modules: vec![String::from("node_modules")],
        node_path: false,
        builtin_modules: false,
        ..ResolveOptions::default()
    }
}

fn parent_dir(path: &Path) -> Result<&Path, VmError> {
    path.parent().ok_or_else(|| VmError::Resolve {
        details: format!("module has no parent directory: {}", path.display()),
    })
}

fn find_tsconfig(start_dir: &Path) -> Option<PathBuf> {
    start_dir
        .ancestors()
        .map(|dir| dir.join("tsconfig.json"))
        .find(|candidate| candidate.is_file())
}

fn project_root(entry_dir: &Path, tsconfig_path: Option<&Path>) -> Result<PathBuf, VmError> {
    let root = match tsconfig_path {
        Some(path) => parent_dir(path)?,
        None => entry_dir,
    };

    root.canonicalize().map_err(VmError::from)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn resolves_extensionless_relative_module() {
        let root = temp_fixture_dir("extensionless");
        let src = root.join("src");
        fs::create_dir_all(&src).expect("create fixture dir");
        fs::write(root.join("main.ts"), "").expect("write main");
        fs::write(src.join("math.ts"), "").expect("write module");

        let resolver = ModuleResolver::for_entry(&root.join("main.ts")).expect("create resolver");
        let resolved = resolver
            .resolve_request(&root.join("main.ts"), "./src/math")
            .expect("resolve module");

        assert_eq!(resolved, src.join("math.ts"));
    }

    #[test]
    fn resolves_index_module() {
        let root = temp_fixture_dir("index");
        let src = root.join("src");
        fs::create_dir_all(&src).expect("create fixture dir");
        fs::write(root.join("main.ts"), "").expect("write main");
        fs::write(src.join("index.ts"), "").expect("write module");

        let resolver = ModuleResolver::for_entry(&root.join("main.ts")).expect("create resolver");
        let resolved = resolver
            .resolve_request(&root.join("main.ts"), "./src")
            .expect("resolve index module");

        assert_eq!(resolved, src.join("index.ts"));
    }

    #[test]
    fn rejects_unresolved_package_requests() {
        let root = temp_fixture_dir("missing-package");
        fs::create_dir_all(&root).expect("create fixture dir");
        fs::write(root.join("main.ts"), "").expect("write main");
        let resolver = ModuleResolver::for_entry(&root.join("main.ts")).expect("create resolver");
        let error = resolver
            .resolve_request(&root.join("main.ts"), "missing-pkg")
            .expect_err("reject unresolved package import");

        assert!(matches!(error, VmError::Resolve { .. }));
    }

    #[test]
    fn resolves_package_import_from_local_node_modules() {
        let root = temp_fixture_dir("package-import");
        let package = root.join("node_modules").join("demo-pkg");
        fs::create_dir_all(&package).expect("create fixture dir");
        fs::write(root.join("main.ts"), "").expect("write main");
        fs::write(
            package.join("package.json"),
            r#"{"name":"demo-pkg","main":"index.ts"}"#,
        )
        .expect("write package manifest");
        fs::write(package.join("index.ts"), "").expect("write package entry");

        let resolver = ModuleResolver::for_entry(&root.join("main.ts")).expect("create resolver");
        let resolved = resolver
            .resolve_request(&root.join("main.ts"), "demo-pkg")
            .expect("resolve package import");

        assert_eq!(resolved, package.join("index.ts"));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_package_import_symlinked_outside_project_root() {
        use std::os::unix::fs::symlink;

        let root = temp_fixture_dir("external-package-symlink");
        let external = temp_fixture_dir("external-package-target");
        let node_modules = root.join("node_modules");
        let package_link = node_modules.join("demo-pkg");
        fs::create_dir_all(&node_modules).expect("create node_modules");
        fs::create_dir_all(&external).expect("create external package");
        fs::write(root.join("main.ts"), "").expect("write main");
        fs::write(
            external.join("package.json"),
            r#"{"name":"demo-pkg","main":"index.ts"}"#,
        )
        .expect("write package manifest");
        fs::write(external.join("index.ts"), "").expect("write package entry");
        symlink(&external, &package_link).expect("symlink external package");

        let resolver = ModuleResolver::for_entry(&root.join("main.ts")).expect("create resolver");
        let error = resolver
            .resolve_request(&root.join("main.ts"), "demo-pkg")
            .expect_err("reject external package symlink");

        assert!(matches!(
            error,
            VmError::Resolve { details }
                if details.contains("outside project root")
        ));
    }

    #[test]
    fn resolves_tsconfig_paths_alias() {
        let root = temp_fixture_dir("tsconfig-alias");
        let src = root.join("src");
        fs::create_dir_all(&src).expect("create fixture dir");
        fs::write(
            root.join("tsconfig.json"),
            r#"{
                "compilerOptions": {
                    "baseUrl": ".",
                    "paths": {
                        "@app/*": ["src/*"]
                    }
                }
            }"#,
        )
        .expect("write tsconfig");
        fs::write(root.join("main.ts"), "").expect("write main");
        fs::write(src.join("math.ts"), "").expect("write module");

        let resolver = ModuleResolver::for_entry(&root.join("main.ts")).expect("create resolver");
        let resolved = resolver
            .resolve_request(&root.join("main.ts"), "@app/math")
            .expect("resolve aliased module");

        assert_eq!(resolved, src.join("math.ts"));
    }

    #[test]
    fn resolves_tsconfig_base_url_request() {
        let root = temp_fixture_dir("tsconfig-base-url");
        let src = root.join("src");
        fs::create_dir_all(&src).expect("create fixture dir");
        fs::write(
            root.join("tsconfig.json"),
            r#"{
                "compilerOptions": {
                    "baseUrl": "."
                }
            }"#,
        )
        .expect("write tsconfig");
        fs::write(root.join("main.ts"), "").expect("write main");
        fs::write(src.join("math.ts"), "").expect("write module");

        let resolver = ModuleResolver::for_entry(&root.join("main.ts")).expect("create resolver");
        let resolved = resolver
            .resolve_request(&root.join("main.ts"), "src/math")
            .expect("resolve baseUrl module");

        assert_eq!(resolved, src.join("math.ts"));
    }

    fn temp_fixture_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "tsvm-resolver-{name}-{}-{nanos}",
            std::process::id()
        ))
    }
}
