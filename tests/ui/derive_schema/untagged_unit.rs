use rustts::TsSchema;

#[derive(TsSchema)]
#[serde(untagged)]
enum BrokenLookup {
    Empty,
    Id(u64),
}

fn main() {}
