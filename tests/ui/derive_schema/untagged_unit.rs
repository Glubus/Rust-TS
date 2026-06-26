use ts_embed_vm::TsSchema;

#[derive(TsSchema)]
#[serde(untagged)]
enum BrokenLookup {
    Empty,
    Id(u64),
}

fn main() {}
