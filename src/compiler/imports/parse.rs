use std::path::Path;

use oxc::allocator::Allocator;
use oxc::ast::ast::Program;
use oxc::parser::Parser;
use oxc::span::SourceType;

use crate::error::VmError;

pub(super) fn with_module_program<T>(
    source_text: &str,
    source_path: &Path,
    action: impl FnOnce(&Program<'_>) -> Result<T, VmError>,
) -> Result<T, VmError> {
    let source_type = module_source_type(source_path)?;
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source_text, source_type).parse();

    reject_parse_diagnostics(&parsed.diagnostics)?;
    action(&parsed.program)
}

fn module_source_type(source_path: &Path) -> Result<SourceType, VmError> {
    SourceType::from_path(source_path)
        .map_err(|error| VmError::Resolve {
            details: error.to_string(),
        })
        .map(|source_type| source_type.with_module(true))
}

/// Syntax errors found while scanning imports are TypeScript errors, not resolution
/// failures, so they surface the same way as errors from the transpiler.
fn reject_parse_diagnostics(diagnostics: &oxc::diagnostics::Diagnostics) -> Result<(), VmError> {
    if diagnostics.is_empty() {
        return Ok(());
    }

    Err(VmError::Transpile {
        details: format!("{diagnostics:?}"),
    })
}
