//! Tokio-backed async host function contract trait.

use std::future::Future;

use super::{HostContract, HostFunctionDescriptor, HostFunctionExecution, Schema};
use crate::error::VmError;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Async host function contract.
#[cfg(feature = "tokio")]
pub trait AsyncHostFunction: HostContract {
    /// Function input type.
    type Input: DeserializeOwned + Send + 'static;
    /// Function output type.
    type Output: Serialize + Send + 'static;
    /// Future returned by the host implementation.
    type Future: Future<Output = Result<Self::Output, VmError>> + Send + 'static;

    /// Returns input schema metadata.
    fn input_schema() -> Schema {
        Self::schema()
    }

    /// Returns output schema metadata.
    fn output_schema() -> Schema {
        Schema::named("unknown")
    }

    /// Executes the async host function from Rust.
    fn call_async(input: Self::Input) -> Self::Future;

    /// Builds function-specific descriptor metadata.
    fn function_descriptor() -> HostFunctionDescriptor {
        HostFunctionDescriptor {
            input_schema: Self::input_schema(),
            output_schema: Self::output_schema(),
            execution: HostFunctionExecution::AsyncBlockingJs,
        }
    }
}
