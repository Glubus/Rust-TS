use rustts::TsSchema;

#[derive(TsSchema)]
union InvalidUnion {
    value: u64,
}

fn main() {}
