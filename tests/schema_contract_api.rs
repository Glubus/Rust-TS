use std::collections::HashMap;

use rustts::{
    HostContract, HostContractKind, HostFunction, InMemoryHostContractRegistry, Schema, TsField,
    TsSchema, TsType, VmError,
};
use serde::{Deserialize, Serialize};

struct FindUsers;

#[derive(Debug, Deserialize)]
struct FindUsersInput {
    ids: Vec<u64>,
}

#[derive(Debug, Serialize)]
struct FindUsersOutput {
    users: HashMap<String, Option<String>>,
}

impl TsSchema for FindUsersInput {
    fn schema_name() -> &'static str {
        "FindUsersInput"
    }

    fn ts_type() -> TsType {
        TsType::Object(vec![TsField::required("ids", Vec::<u64>::ts_type())])
    }
}

impl TsSchema for FindUsersOutput {
    fn schema_name() -> &'static str {
        "FindUsersOutput"
    }

    fn ts_type() -> TsType {
        TsType::Object(vec![TsField::required(
            "users",
            HashMap::<String, Option<String>>::ts_type(),
        )])
    }
}

impl HostContract for FindUsers {
    const NAME: &'static str = "user.findMany";
    const IMPORT_MODULE: &'static str = "test";
    const EXPORT_PATH: &'static [&'static str] = &["user", "findMany"];

    fn schema() -> Schema {
        FindUsersInput::schema()
    }

    fn kind() -> HostContractKind {
        HostContractKind::Function
    }
}

impl HostFunction for FindUsers {
    type Input = FindUsersInput;
    type Output = FindUsersOutput;

    fn input_schema() -> Schema {
        FindUsersInput::schema()
    }

    fn output_schema() -> Schema {
        FindUsersOutput::schema()
    }

    fn call(input: Self::Input) -> Result<Self::Output, VmError> {
        let users = input
            .ids
            .into_iter()
            .map(|id| (id.to_string(), Some(format!("user-{id}"))))
            .collect();
        Ok(FindUsersOutput { users })
    }
}

#[test]
fn host_contracts_can_emit_dts_from_rust_type_schema_trait() {
    let registry = InMemoryHostContractRegistry::new();
    registry.function::<FindUsers>().expect("register function");

    let dts = registry.dts().expect("render declarations");

    assert!(dts.contains("type FindUsersInput = { ids: number[]; };"));
    assert!(dts.contains("type FindUsersOutput = { users: Record<string, string | null>; };"));
    assert!(dts.contains("export function findMany(input: FindUsersInput): FindUsersOutput;"));
}
