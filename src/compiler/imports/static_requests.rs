use std::collections::BTreeSet;

use oxc::ast::ast::{
    ExportAllDeclaration, ExportNamedDeclaration, ImportDeclaration, Program, Statement,
};

pub(super) fn collect_static_module_requests(program: &Program<'_>) -> BTreeSet<String> {
    program
        .body
        .iter()
        .filter_map(static_module_request)
        .map(str::to_owned)
        .collect()
}

fn static_module_request<'a>(statement: &'a Statement<'a>) -> Option<&'a str> {
    match statement {
        Statement::ImportDeclaration(import) => import_request(import),
        Statement::ExportNamedDeclaration(export) => named_export_request(export),
        Statement::ExportAllDeclaration(export) => export_all_request(export),
        _ => None,
    }
}

fn import_request<'a>(import: &'a ImportDeclaration<'a>) -> Option<&'a str> {
    import
        .import_kind
        .is_value()
        .then_some(import.source.value.as_str())
}

fn named_export_request<'a>(export: &'a ExportNamedDeclaration<'a>) -> Option<&'a str> {
    if export.export_kind.is_type() {
        return None;
    }

    let source = export.source.as_ref()?;
    if has_runtime_named_export(export) {
        return Some(source.value.as_str());
    }

    None
}

fn has_runtime_named_export(export: &ExportNamedDeclaration<'_>) -> bool {
    export.specifiers.is_empty()
        || export
            .specifiers
            .iter()
            .any(|specifier| specifier.export_kind.is_value())
}

fn export_all_request<'a>(export: &'a ExportAllDeclaration<'a>) -> Option<&'a str> {
    export
        .export_kind
        .is_value()
        .then_some(export.source.value.as_str())
}
