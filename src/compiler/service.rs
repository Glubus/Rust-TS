//! TypeScript transpilation through oxc.

use std::mem;
use std::path::Path;

use oxc::CompilerInterface;
use oxc::codegen::CodegenReturn;
use oxc::diagnostics::Diagnostics;
use oxc::span::SourceType;
use oxc::transformer::{Module, TransformOptions};

use super::imports::validate_static_module_graph;
use crate::error::VmError;

/// TypeScript to JavaScript compiler configured for runtime execution.
#[derive(Debug, Clone)]
pub(crate) struct CompilerService {
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
    fn execute(
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

    /// Compiles one project module; its source type comes from the file extension.
    pub(crate) fn compile_module(
        &mut self,
        source_text: &str,
        source_path: &Path,
    ) -> Result<String, VmError> {
        let source_type = SourceType::from_path(source_path).map_err(|error| VmError::Resolve {
            details: error.to_string(),
        })?;
        self.execute(source_text, source_type.with_module(true), source_path)
    }

    /// Compiles one inline TypeScript module; it may only import host modules.
    pub(crate) fn compile_inline(
        &mut self,
        source_text: &str,
        source_path: &Path,
    ) -> Result<String, VmError> {
        validate_static_module_graph(source_text, source_path)?;
        self.execute(source_text, SourceType::ts().with_module(true), source_path)
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
