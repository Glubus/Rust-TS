//! Host contract metadata types.

use serde::{Deserialize, Serialize};

/// Delivery behavior for host callbacks.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DeliveryMode {
    /// Deliver to every active binding.
    Broadcast,
    /// Deliver only to the first matching active binding.
    First,
}

/// Minimal schema carrier for V0 metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Schema {
    /// Human-readable schema identity.
    pub name: String,
    /// TypeScript type shape represented by this schema.
    pub ts_type: TsType,
    /// Named schema dependencies referenced by this schema.
    pub dependencies: Vec<Schema>,
}

impl Schema {
    /// Creates a schema with an unknown TypeScript type.
    #[must_use]
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ts_type: TsType::Unknown,
            dependencies: Vec::new(),
        }
    }

    /// Creates a schema backed by a TypeScript type shape.
    #[must_use]
    pub fn typed(name: impl Into<String>, ts_type: TsType) -> Self {
        Self {
            name: name.into(),
            ts_type,
            dependencies: Vec::new(),
        }
    }

    /// Attaches named schema dependencies referenced by this schema.
    #[must_use]
    pub fn with_dependencies(mut self, dependencies: Vec<Schema>) -> Self {
        self.dependencies = dependencies;
        self
    }

    /// Validates one JSON value against this schema.
    ///
    /// Unknown object fields are allowed, matching the default host bridge
    /// validation policy.
    ///
    /// # Errors
    ///
    /// Returns a human-readable validation path and reason when the value does
    /// not match this schema.
    pub fn validate_json(&self, value: &serde_json::Value) -> Result<(), String> {
        super::validation::validate_schema_with_options(
            self,
            value,
            super::validation::SchemaValidationOptions {
                reject_unknown_fields: false,
            },
        )
    }

    /// Validates one JSON value against this schema and rejects unknown object fields.
    ///
    /// # Errors
    ///
    /// Returns a human-readable validation path and reason when the value does
    /// not match this schema or contains undeclared object fields.
    pub fn validate_json_strict(&self, value: &serde_json::Value) -> Result<(), String> {
        super::validation::validate_schema_with_options(
            self,
            value,
            super::validation::SchemaValidationOptions {
                reject_unknown_fields: true,
            },
        )
    }
}

/// TypeScript type shape emitted from host schemas.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum TsType {
    /// Unknown type.
    #[default]
    Unknown,
    /// No value.
    Void,
    /// Boolean value.
    Boolean,
    /// Number value.
    Number,
    /// String value.
    String,
    /// Arbitrary JSON-compatible value.
    Json,
    /// Null value.
    Null,
    /// Named TypeScript type reference.
    TypeRef(String),
    /// Literal TypeScript value.
    Literal(TsLiteral),
    /// Object type with named fields.
    Object(Vec<TsField>),
    /// Array type.
    Array(Box<TsType>),
    /// Tuple type.
    Tuple(Vec<TsType>),
    /// Rust-like enum type.
    Enum {
        /// Discriminant field used when variants carry payload fields.
        tag: Option<String>,
        /// Enum variants.
        variants: Vec<TsEnumVariant>,
    },
    /// Record type.
    Record {
        /// Record key type.
        key: TsRecordKey,
        /// Record value type.
        value: Box<TsType>,
    },
    /// Union type.
    Union(Vec<TsType>),
    /// Optional type.
    Optional(Box<TsType>),
    /// Nullable type.
    Nullable(Box<TsType>),
}

/// Literal TypeScript value emitted from a schema.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TsLiteral {
    /// String literal.
    String(String),
    /// Number literal stored as source text.
    Number(String),
    /// Boolean literal.
    Boolean(bool),
}

/// Variant of a TypeScript enum model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TsEnumVariant {
    /// Variant name.
    pub name: String,
    /// Payload fields carried by this variant.
    pub fields: Vec<TsField>,
}

impl TsEnumVariant {
    /// Creates a unit enum variant.
    #[must_use]
    pub fn unit(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            fields: Vec::new(),
        }
    }

    /// Creates an enum variant with object payload fields.
    #[must_use]
    pub fn payload(name: impl Into<String>, fields: Vec<TsField>) -> Self {
        Self {
            name: name.into(),
            fields,
        }
    }
}

/// TypeScript record key category.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TsRecordKey {
    /// String keys.
    String,
    /// Number keys.
    Number,
}

/// Object field in a TypeScript schema.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TsField {
    /// Field name.
    pub name: String,
    /// Field type.
    pub ty: TsType,
    /// Whether this field is optional.
    pub optional: bool,
}

impl TsField {
    /// Creates a required field.
    #[must_use]
    pub fn required(name: impl Into<String>, ty: TsType) -> Self {
        Self {
            name: name.into(),
            ty,
            optional: false,
        }
    }

    /// Creates an optional field.
    #[must_use]
    pub fn optional(name: impl Into<String>, ty: TsType) -> Self {
        Self {
            name: name.into(),
            ty,
            optional: true,
        }
    }
}

/// Shared host contract metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostMetadata {
    /// Stable contract name.
    pub name: String,
    /// Free-form tags.
    pub tags: Vec<String>,
}

/// ESM import binding exposed by the host module loader.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostImportBinding {
    /// Virtual ESM module name, such as `"oppw4"`.
    pub module: String,
    /// Export path inside the module, such as `["character", "find"]`.
    pub export_path: Vec<String>,
}

/// Top-level contract category.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum HostContractKind {
    /// Function contract.
    Function,
    /// Callback contract.
    Callback,
    /// Context contract.
    Context,
}

/// Runtime and typing descriptor stored by the host contract registry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostContractDescriptor {
    /// Stable contract name.
    pub name: String,
    /// Contract category.
    pub kind: HostContractKind,
    /// Schema metadata.
    pub schema: Schema,
    /// Extra metadata.
    pub metadata: HostMetadata,
    /// Required ESM import binding for scripts.
    pub import: HostImportBinding,
    /// Callback-specific routing metadata.
    pub callback: Option<HostCallbackDescriptor>,
    /// Function-specific call metadata.
    pub function: Option<HostFunctionDescriptor>,
    /// Normalized ABI contract model derived from Rust traits.
    pub abi: HostContractAbi,
}

/// Runtime metadata specific to callback contracts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostCallbackDescriptor {
    /// Payload schema metadata.
    pub payload_schema: Schema,
    /// Delivery behavior for this callback.
    pub delivery: DeliveryMode,
    /// Whether this callback is expected on the hot path.
    pub hot: bool,
}

/// Runtime and typing metadata specific to function contracts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostFunctionDescriptor {
    /// Input schema metadata.
    pub input_schema: Schema,
    /// Output schema metadata.
    pub output_schema: Schema,
    /// Execution mode exposed by this function binding.
    pub execution: HostFunctionExecution,
}

/// Host function execution mode.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum HostFunctionExecution {
    /// Synchronous Rust function and synchronous JavaScript bridge call.
    Sync,
    /// Rust future executed on Tokio, with the current JavaScript bridge waiting for completion.
    AsyncBlockingJs,
    /// Future non-blocking JavaScript promise bridge.
    AsyncPromise,
}

/// Normalized ABI data for one host contract.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum HostContractAbi {
    /// Function ABI.
    Function {
        /// Input schema.
        input: Schema,
        /// Output schema.
        output: Schema,
        /// Function execution mode.
        execution: HostFunctionExecution,
    },
    /// Callback ABI.
    Callback {
        /// Payload schema.
        payload: Schema,
        /// Delivery behavior for this callback.
        delivery: DeliveryMode,
        /// Whether this callback is expected on the hot path.
        hot: bool,
    },
    /// Context ABI.
    Context {
        /// Context schema.
        schema: Schema,
    },
    /// ABI has not been specialized yet.
    Unknown,
}
