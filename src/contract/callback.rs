//! Host callback contract trait.

use super::{HostCallbackDescriptor, HostContract, Schema};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Host callback contract.
pub trait HostCallback: HostContract {
    /// Callback payload type.
    type Payload: Serialize + DeserializeOwned;

    /// Returns payload schema metadata.
    fn payload_schema() -> Schema {
        Self::schema()
    }

    /// Builds callback-specific descriptor metadata.
    fn callback_descriptor() -> HostCallbackDescriptor {
        HostCallbackDescriptor {
            payload_schema: Self::payload_schema(),
        }
    }
}
