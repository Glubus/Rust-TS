use ts_embed_vm::TsSchema;

#[derive(TsSchema)]
struct Inner {
    value: String,
}

#[derive(TsSchema)]
struct InvalidFlatten(#[serde(flatten)] Inner);

fn main() {}
