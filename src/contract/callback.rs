//! Host callback contract traits.

use super::HostContract;

/// Host callback contract: an event the host emits to the scripts' `ctx.on` handlers.
pub trait HostCallback: HostContract {
    /// Callback payload type.
    type Payload;
}

/// Host callback whose handlers answer: [`Engine::request`](crate::Engine::request)
/// returns what each handler returns. Register it with
/// [`request`](crate::InMemoryHostContractRegistry::request) so the
/// generated TypeScript types its handlers as returning `Reply`.
pub trait HostRequest: HostCallback {
    /// What one handler returns, or what the Promise an `async` handler returns
    /// resolves to.
    type Reply;
}
