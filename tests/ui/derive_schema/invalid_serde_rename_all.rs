use ts_embed_vm::TsSchema;

#[derive(TsSchema)]
#[serde(rename_all = "mixedCase")]
struct InvalidSerdeRenameAll {
    value_id: u64,
}

fn main() {}
