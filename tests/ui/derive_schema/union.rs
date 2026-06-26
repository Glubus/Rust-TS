use ts_embed_vm::TsSchema;

#[derive(TsSchema)]
union InvalidUnion {
    value: u64,
}

fn main() {}
