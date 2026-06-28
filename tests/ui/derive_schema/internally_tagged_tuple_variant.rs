use ts_embed_vm::TsSchema;

#[derive(TsSchema)]
#[serde(tag = "kind")]
enum HostEvent {
    Data(String),
}

fn main() {}
