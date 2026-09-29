//! Host function contract traits.

use super::{HostContract, HostFunctionDescriptor, Schema};
use crate::error::VmError;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Input and output of a host function: what scripts see, whatever implements it.
///
/// A contract implementing only this trait is registered together with a handler
/// value, such as a closure holding state
/// ([`InMemoryHostContractRegistry::typed_function_with`](crate::InMemoryHostContractRegistry::typed_function_with));
/// a contract implementing [`HostFunction`] as well is registered on its own.
pub trait HostFunctionSignature: HostContract {
    /// Function input type.
    type Input: DeserializeOwned;
    /// Function output type.
    type Output: Serialize;

    /// Returns input schema metadata.
    fn input_schema() -> Schema {
        Self::schema()
    }

    /// Returns output schema metadata.
    fn output_schema() -> Schema {
        Schema::named("unknown")
    }

    /// Builds function-specific descriptor metadata.
    fn function_descriptor() -> HostFunctionDescriptor {
        HostFunctionDescriptor {
            input_schema: Self::input_schema(),
            output_schema: Self::output_schema(),
            returns_promise: false,
        }
    }
}

/// Host function implemented by a static Rust function.
pub trait HostFunction: HostFunctionSignature {
    /// Executes the host function from Rust.
    fn call(input: Self::Input) -> Result<Self::Output, VmError>;
}
