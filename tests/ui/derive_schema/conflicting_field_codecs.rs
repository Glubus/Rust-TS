use rustts::TsSchema;

#[derive(TsSchema)]
struct Conflict {
    #[rustts(codec = "json", with = "custom")]
    value: String,
}

fn main() {}
