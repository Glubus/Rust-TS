use rustts::TsSchema;

#[derive(TsSchema)]
#[serde(tag = "kind")]
enum HostEvent {
    Data(String, u32),
}

fn main() {}
