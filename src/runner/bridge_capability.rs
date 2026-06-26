//! Worker bridge capability checks.

use crate::contract::{HostContractKind, HostFunctionExecution};
use crate::error::VmError;
use crate::registry::InMemoryHostContractRegistry;

/// Capabilities provided by one worker's JavaScript host bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkerBridgeCapability {
    /// Synchronous QuickJS context and synchronous host bridge.
    Sync,
}

impl WorkerBridgeCapability {
    pub(crate) fn supports_function_execution(self, execution: HostFunctionExecution) -> bool {
        match self {
            Self::Sync => execution != HostFunctionExecution::AsyncPromise,
        }
    }
}

pub(crate) fn ensure_host_contracts_supported(
    host_registry: &InMemoryHostContractRegistry,
    capability: WorkerBridgeCapability,
) -> Result<(), VmError> {
    for descriptor in host_registry.descriptors()? {
        if descriptor.kind != HostContractKind::Function {
            continue;
        }

        let Some(function) = descriptor.function.as_ref() else {
            continue;
        };

        if !capability.supports_function_execution(function.execution) {
            return Err(VmError::UnsupportedHostBridge {
                contract_name: descriptor.name,
                execution: function.execution,
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_worker_supports_direct_return_function_modes_only() {
        let capability = WorkerBridgeCapability::Sync;

        assert!(capability.supports_function_execution(HostFunctionExecution::Sync));
        assert!(capability.supports_function_execution(HostFunctionExecution::AsyncBlockingJs));
        assert!(!capability.supports_function_execution(HostFunctionExecution::AsyncPromise));
    }
}
