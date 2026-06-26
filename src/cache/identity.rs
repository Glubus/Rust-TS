//! Versioned cache identity for compiled script artifacts.

use serde::Serialize;

const CACHE_SCHEMA_VERSION: &str = "1";
const AUTHOR_MODULE_BRIDGE_VERSION: &str = "native-esm-v1";
const COMPILER_MODULE_FORMAT: &str = "esm";
const OXC_VERSION: &str = "0.137.0";
const OXC_RESOLVER_VERSION: &str = "11.21.3";
const RQUICKJS_VERSION: &str = "0.12.0";
const RUNTIME_BRIDGE_VERSION: &str = "quickjs-native-esm-loader-v1";
const RESOLVER_POLICY_VERSION: &str = "local-relative-package-index-v2";

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CacheIdentity<'a> {
    schema_version: &'static str,
    artifact_kind: CacheArtifactKind,
    source_seed: &'a str,
    compiler: CompilerIdentity,
    resolver: ResolverIdentity,
    runtime: RuntimeIdentity,
    host_contract_abi: &'a str,
}

impl<'a> CacheIdentity<'a> {
    pub(crate) fn inline(source_seed: &'a str, host_contract_abi: &'a str) -> Self {
        Self::new(CacheArtifactKind::Inline, source_seed, host_contract_abi)
    }

    pub(crate) fn project(source_seed: &'a str, host_contract_abi: &'a str) -> Self {
        Self::new(CacheArtifactKind::Project, source_seed, host_contract_abi)
    }

    fn new(
        artifact_kind: CacheArtifactKind,
        source_seed: &'a str,
        host_contract_abi: &'a str,
    ) -> Self {
        Self {
            schema_version: CACHE_SCHEMA_VERSION,
            artifact_kind,
            source_seed,
            compiler: CompilerIdentity::current(),
            resolver: ResolverIdentity::current(),
            runtime: RuntimeIdentity::current(),
            host_contract_abi,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
enum CacheArtifactKind {
    Inline,
    Project,
}

#[derive(Debug, Clone, Serialize)]
struct CompilerIdentity {
    oxc_version: &'static str,
    module_format: &'static str,
    author_module_bridge: &'static str,
}

impl CompilerIdentity {
    fn current() -> Self {
        Self {
            oxc_version: OXC_VERSION,
            module_format: COMPILER_MODULE_FORMAT,
            author_module_bridge: AUTHOR_MODULE_BRIDGE_VERSION,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct ResolverIdentity {
    oxc_resolver_version: &'static str,
    policy: &'static str,
}

impl ResolverIdentity {
    fn current() -> Self {
        Self {
            oxc_resolver_version: OXC_RESOLVER_VERSION,
            policy: RESOLVER_POLICY_VERSION,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct RuntimeIdentity {
    crate_version: &'static str,
    rquickjs_version: &'static str,
    bridge_version: &'static str,
}

impl RuntimeIdentity {
    fn current() -> Self {
        Self {
            crate_version: env!("CARGO_PKG_VERSION"),
            rquickjs_version: RQUICKJS_VERSION,
            bridge_version: RUNTIME_BRIDGE_VERSION,
        }
    }
}
