//! Host callback contract trait.

use super::{DeliveryMode, HostCallbackDescriptor, HostContract, Schema};
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

    /// Delivery mode for the callback.
    fn delivery() -> DeliveryMode {
        DeliveryMode::Broadcast
    }

    /// Whether this callback belongs to the hot path.
    fn hot() -> bool {
        true
    }

    /// Builds callback-specific descriptor metadata.
    fn callback_descriptor() -> HostCallbackDescriptor {
        HostCallbackDescriptor {
            payload_schema: Self::payload_schema(),
            delivery: Self::delivery(),
            hot: Self::hot(),
        }
    }
}
