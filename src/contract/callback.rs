//! Host callback contract traits.

use super::{HostCallbackDescriptor, HostContract, Schema};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Host callback contract: an event the host emits to the scripts' `ctx.on` handlers.
pub trait HostCallback: HostContract {
    /// Callback payload type.
    type Payload: Serialize + DeserializeOwned;

    /// Returns payload schema metadata.
    fn payload_schema() -> Schema {
        Self::schema()
    }

    /// Builds callback-specific descriptor metadata. Handlers of a plain callback
    /// return nothing, so it has no reply schema.
    fn callback_descriptor() -> HostCallbackDescriptor {
        HostCallbackDescriptor {
            payload_schema: Self::payload_schema(),
            reply_schema: None,
        }
    }
}

/// Host callback whose handlers answer: [`Engine::request`](crate::Engine::request)
/// returns what each handler returns. Register it with
/// [`typed_request`](crate::InMemoryHostContractRegistry::typed_request) so the
/// generated TypeScript types its handlers as returning `Reply`.
pub trait HostRequest: HostCallback {
    /// What one handler returns, or what the Promise an `async` handler returns
    /// resolves to.
    type Reply;
}
