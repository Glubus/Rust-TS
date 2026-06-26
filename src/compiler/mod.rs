//! Control-plane TypeScript compiler service.

mod imports;
mod project;
mod resolver;
mod service;

pub(crate) use imports::validate_static_module_graph;
pub(crate) use project::{CompiledModule, ProjectCompileOutput};
pub(crate) use service::CompiledScript;
pub use service::CompilerService;
