//! Host function contract trait.

use super::{HostContract, HostFunctionDescriptor, Schema};
use crate::error::VmError;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Host function contract.
pub trait HostFunction: HostContract {
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

    /// Executes the host function from Rust.
    fn call(input: Self::Input) -> Result<Self::Output, VmError>;

    /// Builds function-specific descriptor metadata.
    fn function_descriptor() -> HostFunctionDescriptor {
        HostFunctionDescriptor {
            input_schema: Self::input_schema(),
            output_schema: Self::output_schema(),
        }
    }
}
