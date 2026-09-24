//! Native codecs and schemas for popular third-party crates, each behind the Cargo
//! feature of the same name. Every type crosses exactly as its own `serde`
//! implementation does through `serde_json`, under the same number rules as the
//! standard types.

#[cfg(feature = "chrono")]
mod chrono;
#[cfg(feature = "glam")]
mod glam;
#[cfg(feature = "uuid")]
mod uuid;
