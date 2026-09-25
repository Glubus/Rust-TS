//! TypeScript compilation: transpilation and static ESM project graphs.

mod imports;
mod project;
mod resolver;
mod service;

pub(crate) use project::{CompiledModule, ProjectCompileOutput, discover_project};
pub(crate) use service::CompilerService;
