use rustts::TsSchema;

#[derive(TsSchema)]
#[serde(tag = "type")]
struct Tagged {
    value: u32,
}

fn main() {}
