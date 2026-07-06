//! Compiler service and artifact assembly.

use std::collections::BTreeSet;
use std::mem;
use std::path::{Path, PathBuf};

use oxc::CompilerInterface;
use oxc::codegen::CodegenReturn;
use oxc::diagnostics::Diagnostics;
use oxc::span::SourceType;
use oxc::transformer::{Module, TransformOptions};

use super::imports::validate_static_module_graph;
use super::project::{ProjectCompileOutput, compile_project};
use crate::error::VmError;

/// Compiled JavaScript artifact prepared by the control plane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompiledScript {
    pub(crate) cache_key: String,
    pub(crate) transpiled_js: String,
    pub(crate) transpiled_path: PathBuf,
    pub(crate) entry_path: Option<String>,
    pub(crate) modules: Vec<super::project::CompiledModule>,
}

/// TypeScript to JavaScript compiler configured for runtime execution.
#[derive(Debug, Clone)]
pub struct CompilerService {
    printed: String,
    errors: Diagnostics,
    transform_options: TransformOptions,
}

impl Default for CompilerService {
    fn default() -> Self {
        let mut transform_options = TransformOptions::default();
        transform_options.env.module = Module::Esm;

        Self {
            printed: String::new(),
            errors: Diagnostics::default(),
            transform_options,
        }
    }
}

impl CompilerService {
    /// Compiles one TypeScript source into executable JavaScript.
    pub fn execute(
        &mut self,
        source_text: &str,
        source_type: SourceType,
        source_path: &Path,
    ) -> Result<String, VmError> {
        self.compile(source_text, source_type, source_path);

        if self.errors.is_empty() {
            return Ok(mem::take(&mut self.printed));
        }

        Err(VmError::Transpile {
            details: format!("{:?}", mem::take(&mut self.errors)),
        })
    }

    pub(crate) fn execute_module(
        &mut self,
        source_text: &str,
        source_path: &Path,
    ) -> Result<String, VmError> {
        let source_type = SourceType::from_path(source_path).map_err(|error| VmError::Resolve {
            details: error.to_string(),
        })?;
        self.execute(source_text, source_type.with_module(true), source_path)
    }

    /// Compiles TypeScript source and builds the cache artifact description.
    pub(crate) fn compile_script(
        &mut self,
        cache_key: String,
        source_text: &str,
        source_path: &Path,
        transpiled_path: PathBuf,
    ) -> Result<CompiledScript, VmError> {
        validate_static_module_graph(source_text, source_path)?;
        let transpiled_js =
            self.execute(source_text, SourceType::ts().with_module(true), source_path)?;
        Ok(CompiledScript {
            cache_key,
            transpiled_js,
            transpiled_path,
            entry_path: None,
            modules: Vec::new(),
        })
    }

    pub(crate) fn compile_project(
        &mut self,
        entry_path: &Path,
        external_modules: &BTreeSet<String>,
    ) -> Result<ProjectCompileOutput, VmError> {
        compile_project(self, entry_path, external_modules)
    }

    pub(crate) fn build_project_script(
        &mut self,
        cache_key: String,
        transpiled_path: PathBuf,
        project: &ProjectCompileOutput,
    ) -> Result<CompiledScript, VmError> {
        Ok(CompiledScript {
            cache_key,
            transpiled_js: String::new(),
            transpiled_path,
            entry_path: Some(project.entry_module_id.clone()),
            modules: project.modules.clone(),
        })
    }
}

impl CompilerInterface for CompilerService {
    fn handle_errors(&mut self, errors: Diagnostics) {
        self.errors.extend(errors);
    }

    fn transform_options(&self) -> Option<&TransformOptions> {
        Some(&self.transform_options)
    }

    fn after_codegen(&mut self, ret: CodegenReturn<'_>) {
        self.printed = ret.code;
    }
}
