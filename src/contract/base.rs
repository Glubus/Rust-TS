//! Base host contract trait.

use super::{HostContractAbi, HostContractDescriptor, HostContractKind, HostMetadata, Schema};

/// Base contract trait. Traits produce the contract model, not just runtime hooks.
pub trait HostContract {
    /// Stable contract identity.
    const NAME: &'static str;

    /// Returns the schema metadata of this contract.
    fn schema() -> Schema;

    /// Returns additional host metadata.
    fn metadata() -> HostMetadata {
        HostMetadata {
            name: Self::NAME.to_owned(),
            tags: Vec::new(),
        }
    }

    /// Returns the category of this contract.
    fn kind() -> HostContractKind;

    /// Builds the base descriptor stored in registries.
    fn descriptor() -> HostContractDescriptor {
        HostContractDescriptor {
            name: Self::NAME.to_owned(),
            kind: Self::kind(),
            schema: Self::schema(),
            metadata: Self::metadata(),
            callback: None,
            function: None,
            abi: HostContractAbi::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{HostContractKind, Schema};

    struct DemoContract;

    impl HostContract for DemoContract {
        const NAME: &'static str = "demo.contract";

        fn schema() -> Schema {
            Schema::named("DemoSchema")
        }

        fn kind() -> HostContractKind {
            HostContractKind::Function
        }
    }

    #[test]
    fn descriptor_uses_contract_metadata() {
        let descriptor = DemoContract::descriptor();

        assert_eq!(descriptor.name, "demo.contract");
        assert_eq!(descriptor.kind, HostContractKind::Function);
        assert_eq!(descriptor.schema.name, "DemoSchema");
    }
}
