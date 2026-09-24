use rustts::TsSchema;

#[derive(TsSchema)]
#[rustts(encode_only, decode_only)]
struct Conflict {
    value: String,
}

fn main() {}
