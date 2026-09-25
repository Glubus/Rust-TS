//! TypeScript compilation: transpilation and static ESM project graphs.

mod imports;
mod project;
mod resolver;
mod service;
mod stamp;

pub(crate) use imports::extract_static_import_requests;
pub(crate) use project::{CompiledModule, ProjectState, discover_project};
pub(crate) use service::CompilerService;
pub(crate) use stamp::WatchedFiles;
