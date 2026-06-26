#![cfg(feature = "derive")]

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::net::IpAddr;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_embed_vm::{
    HostContract, HostContractKind, HostFunction, InMemoryHostContractRegistry, Schema,
    TsEnumVariant, TsField, TsRecordKey, TsSchema, TsType, VmError,
};

#[derive(TsSchema)]
#[allow(dead_code)]
struct InvoicePayload {
    id: u64,
    #[tsvm(rename = "displayName")]
    name: String,
    #[tsvm(optional)]
    metadata: BTreeMap<String, String>,
    flags: Vec<bool>,
}

#[derive(TsSchema)]
#[tsvm(name = "BillingStatus")]
#[allow(dead_code)]
enum Status {
    Pending,
    Paid,
}

#[derive(TsSchema)]
#[allow(dead_code)]
struct Point(f32, f32);

#[derive(Debug, Deserialize, TsSchema)]
#[serde(transparent)]
#[allow(dead_code)]
struct UserId(u64);

#[derive(Debug, Serialize, TsSchema)]
#[serde(transparent)]
#[allow(dead_code)]
struct SessionToken {
    value: String,
}

struct FindSession;

#[derive(Debug, Deserialize, TsSchema)]
#[allow(dead_code)]
struct FindSessionInput {
    user_id: UserId,
}

#[derive(Debug, Serialize, TsSchema)]
#[allow(dead_code)]
struct FindSessionOutput {
    token: SessionToken,
}

#[derive(TsSchema)]
#[allow(dead_code)]
struct FixedPayload {
    color: [u8; 4],
    points: [[f32; 2]; 2],
}

#[derive(TsSchema)]
#[allow(dead_code)]
struct IndexedPayload {
    users_by_id: HashMap<u64, String>,
    damage_by_source: BTreeMap<i32, f32>,
}

#[derive(TsSchema)]
#[allow(dead_code)]
struct SetPayload {
    tags: HashSet<String>,
    sorted_scores: BTreeSet<u64>,
}

#[derive(TsSchema)]
#[allow(dead_code)]
struct HostResourcePayload {
    path: PathBuf,
    bind_addr: IpAddr,
}

#[derive(TsSchema)]
#[allow(dead_code)]
struct RecursiveNode {
    child: Option<Box<RecursiveNode>>,
}

#[derive(TsSchema)]
#[allow(dead_code)]
struct Envelope<T> {
    payload: T,
}

#[derive(TsSchema)]
#[allow(dead_code)]
enum HostMessage<T> {
    Ready,
    User { id: u64, name: String },
    Data(T),
    Pair(String, bool),
}

#[derive(TsSchema)]
#[serde(untagged)]
#[allow(dead_code)]
enum LookupInput {
    Id(u64),
    Query {
        #[serde(rename = "accountId")]
        account_id: String,
        include_disabled: bool,
    },
    Tags(Vec<String>),
}

struct CreateInvoice;

#[derive(Debug, Deserialize, TsSchema)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct CreateInvoiceInput {
    account_id: String,
    total: f32,
    memo: Option<String>,
    #[serde(default)]
    retry_count: u32,
}

#[derive(Debug, Serialize, TsSchema)]
#[tsvm(name = "InvoiceCreated")]
#[allow(dead_code)]
struct CreateInvoiceOutput {
    invoice_id: String,
    accepted: bool,
}

impl HostContract for CreateInvoice {
    const NAME: &'static str = "billing.invoice.create";

    fn schema() -> Schema {
        CreateInvoiceInput::schema()
    }

    fn kind() -> HostContractKind {
        HostContractKind::Function
    }
}

impl HostFunction for CreateInvoice {
    type Input = CreateInvoiceInput;
    type Output = CreateInvoiceOutput;

    fn input_schema() -> Schema {
        CreateInvoiceInput::schema()
    }

    fn output_schema() -> Schema {
        CreateInvoiceOutput::schema()
    }

    fn call(input: Self::Input) -> Result<Self::Output, VmError> {
        Ok(CreateInvoiceOutput {
            invoice_id: format!("invoice:{}", input.account_id),
            accepted: input.total > 0.0,
        })
    }
}

impl HostContract for FindSession {
    const NAME: &'static str = "session.find";

    fn schema() -> Schema {
        FindSessionInput::schema()
    }

    fn kind() -> HostContractKind {
        HostContractKind::Function
    }
}

impl HostFunction for FindSession {
    type Input = FindSessionInput;
    type Output = FindSessionOutput;

    fn input_schema() -> Schema {
        FindSessionInput::schema()
    }

    fn output_schema() -> Schema {
        FindSessionOutput::schema()
    }

    fn call(input: Self::Input) -> Result<Self::Output, VmError> {
        Ok(FindSessionOutput {
            token: SessionToken {
                value: format!("session:{}", input.user_id.0),
            },
        })
    }
}

#[derive(TsSchema)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct SerdePlayerPayload {
    player_id: u64,
    critical_hits: u32,
    nickname: std::option::Option<String>,
    #[serde(default)]
    lives: u32,
    #[serde(default = "default_spawn_region")]
    spawn_region: String,
    #[serde(rename = "damage")]
    amount: f32,
    #[tsvm(rename = "tsvmName")]
    serde_priority_check: String,
    #[serde(skip)]
    internal_seed: u64,
}

#[allow(dead_code)]
fn default_spawn_region() -> String {
    String::from("arena")
}

#[derive(TsSchema)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
enum SerdeMode {
    FastMode,
    #[serde(rename = "manual-mode")]
    SlowMode,
}

#[test]
fn derive_ts_schema_for_named_struct_fields() {
    assert_eq!(InvoicePayload::schema_name(), "InvoicePayload");
    assert_eq!(
        InvoicePayload::ts_type(),
        TsType::Object(vec![
            TsField::required("id", TsType::Number),
            TsField::required("displayName", TsType::String),
            TsField::optional(
                "metadata",
                TsType::Record {
                    key: TsRecordKey::String,
                    value: Box::new(TsType::String),
                },
            ),
            TsField::required("flags", TsType::Array(Box::new(TsType::Boolean))),
        ])
    );
}

#[test]
fn derive_ts_schema_respects_serde_field_attributes() {
    assert_eq!(
        SerdePlayerPayload::ts_type(),
        TsType::Object(vec![
            TsField::required("playerId", TsType::Number),
            TsField::required("criticalHits", TsType::Number),
            TsField::optional("nickname", TsType::Nullable(Box::new(TsType::String))),
            TsField::optional("lives", TsType::Number),
            TsField::optional("spawnRegion", TsType::String),
            TsField::required("damage", TsType::Number),
            TsField::required("tsvmName", TsType::String),
        ])
    );
}

#[test]
fn derive_ts_schema_respects_serde_enum_variant_attributes() {
    assert_eq!(
        SerdeMode::ts_type(),
        TsType::Enum {
            tag: Some(String::from("type")),
            variants: vec![
                TsEnumVariant::unit("fast-mode"),
                TsEnumVariant::unit("manual-mode"),
            ],
        }
    );
}

#[test]
fn derive_ts_schema_for_unit_enum() {
    assert_eq!(Status::schema_name(), "BillingStatus");
    assert!(matches!(Status::ts_type(), TsType::Enum { .. }));
}

#[test]
fn derive_ts_schema_for_tuple_struct() {
    assert_eq!(
        Point::ts_type(),
        TsType::Tuple(vec![TsType::Number, TsType::Number])
    );
}

#[test]
fn derive_ts_schema_for_serde_transparent_newtypes() {
    assert_eq!(UserId::schema_name(), "UserId");
    assert_eq!(UserId::ts_type(), TsType::Number);
    assert_eq!(SessionToken::schema_name(), "SessionToken");
    assert_eq!(SessionToken::ts_type(), TsType::String);
}

#[test]
fn derive_ts_schema_for_fixed_array_fields() {
    assert_eq!(
        FixedPayload::ts_type(),
        TsType::Object(vec![
            TsField::required("color", TsType::Array(Box::new(TsType::Number))),
            TsField::required(
                "points",
                TsType::Array(Box::new(TsType::Array(Box::new(TsType::Number)))),
            ),
        ])
    );
}

#[test]
fn derive_ts_schema_for_numeric_record_fields() {
    assert_eq!(
        IndexedPayload::ts_type(),
        TsType::Object(vec![
            TsField::required(
                "users_by_id",
                TsType::Record {
                    key: TsRecordKey::Number,
                    value: Box::new(TsType::String),
                },
            ),
            TsField::required(
                "damage_by_source",
                TsType::Record {
                    key: TsRecordKey::Number,
                    value: Box::new(TsType::Number),
                },
            ),
        ])
    );
}

#[test]
fn derive_ts_schema_for_set_fields() {
    assert_eq!(
        SetPayload::ts_type(),
        TsType::Object(vec![
            TsField::required("tags", TsType::Array(Box::new(TsType::String))),
            TsField::required("sorted_scores", TsType::Array(Box::new(TsType::Number))),
        ])
    );
}

#[test]
fn derive_ts_schema_for_std_string_like_fields() {
    assert_eq!(
        HostResourcePayload::ts_type(),
        TsType::Object(vec![
            TsField::required("path", TsType::String),
            TsField::required("bind_addr", TsType::String),
        ])
    );
}

#[test]
fn derive_ts_schema_for_recursive_box_field() {
    assert_eq!(
        RecursiveNode::ts_type(),
        TsType::Object(vec![TsField::optional(
            "child",
            TsType::Nullable(Box::new(TsType::TypeRef(String::from("RecursiveNode")))),
        )])
    );

    let schema = RecursiveNode::schema();

    assert_eq!(schema.name, "RecursiveNode");
    assert_eq!(schema.dependencies.len(), 1);
    assert_eq!(schema.dependencies[0].name, "RecursiveNode");
}

#[test]
fn derive_ts_schema_for_generic_struct() {
    assert_eq!(
        Envelope::<String>::ts_type(),
        TsType::Object(vec![TsField::required("payload", TsType::String)])
    );
}

#[test]
fn derive_ts_schema_for_payload_enum() {
    assert_eq!(
        HostMessage::<String>::ts_type(),
        TsType::Enum {
            tag: Some(String::from("type")),
            variants: vec![
                TsEnumVariant::unit("Ready"),
                TsEnumVariant::payload(
                    "User",
                    vec![
                        TsField::required("id", TsType::Number),
                        TsField::required("name", TsType::String),
                    ],
                ),
                TsEnumVariant::payload("Data", vec![TsField::required("value", TsType::String)]),
                TsEnumVariant::payload(
                    "Pair",
                    vec![TsField::required(
                        "items",
                        TsType::Tuple(vec![TsType::String, TsType::Boolean]),
                    )],
                ),
            ],
        }
    );
}

#[test]
fn derive_ts_schema_for_serde_untagged_enum() {
    assert_eq!(
        LookupInput::ts_type(),
        TsType::Union(vec![
            TsType::Number,
            TsType::Object(vec![
                TsField::required("accountId", TsType::String),
                TsField::required("include_disabled", TsType::Boolean),
            ]),
            TsType::Array(Box::new(TsType::String)),
        ])
    );
}

#[test]
fn derived_schema_can_drive_host_contract_dts() {
    let registry = InMemoryHostContractRegistry::new();
    registry
        .function::<CreateInvoice>()
        .expect("register derived-schema host function");

    let dts = registry.dts().expect("render derived-schema declarations");

    assert!(dts.contains(
        "type CreateInvoiceInput = { accountId: string; total: number; memo?: string | null; retryCount?: number; };"
    ));
    assert!(dts.contains("type InvoiceCreated = { invoice_id: string; accepted: boolean; };"));
    assert!(dts.contains(
        "declare namespace billing {\n  namespace invoice {\n    export function create(input: CreateInvoiceInput): InvoiceCreated;\n  }\n}"
    ));
}

#[test]
fn transparent_newtypes_drive_host_contract_dts() {
    let registry = InMemoryHostContractRegistry::new();
    registry
        .function::<FindSession>()
        .expect("register transparent-newtype host function");

    let dts = registry
        .dts()
        .expect("render transparent-newtype declarations");

    assert!(dts.contains("type UserId = number;"));
    assert!(dts.contains("type SessionToken = string;"));
    assert!(dts.contains("type FindSessionInput = { user_id: UserId; };"));
    assert!(dts.contains("type FindSessionOutput = { token: SessionToken; };"));
    assert!(dts.contains(
        "declare namespace session {\n  export function find(input: FindSessionInput): FindSessionOutput;\n}"
    ));

    let sdk = registry
        .sdk()
        .expect("render transparent-newtype SDK declarations");

    assert!(sdk.contains("type UserId = number;"));
    assert!(sdk.contains("type SessionToken = string;"));
    assert!(sdk.contains("type FindSessionInput = { user_id: UserId; };"));
    assert!(sdk.contains("type FindSessionOutput = { token: SessionToken; };"));
}
