use ts_embed_vm::TsSchema;

#[derive(TsSchema)]
#[serde(transparent)]
struct InvalidTransparent {
    id: u64,
    name: String,
}

fn main() {}
