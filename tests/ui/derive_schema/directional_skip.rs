use rustts::TsSchema;

#[derive(TsSchema)]
struct DirectionalSkip {
    visible: String,
    #[serde(skip_serializing)]
    input_only: String,
}

fn main() {}
