use ts_embed_vm::TsSchema;

#[derive(TsSchema)]
struct DirectionalSkip {
    visible: String,
    #[serde(skip_serializing)]
    input_only: String,
}

fn main() {}
