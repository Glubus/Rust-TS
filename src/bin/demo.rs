use ts_embed_vm::{TsVm, VmOptions};

const DEMO_SCRIPT: &str = include_str!("../../assets/demo_math.ts");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let vm = TsVm::new(VmOptions {
        worker_threads: 0,
        ..VmOptions::default()
    })?;
    let subscription = vm.subscribe();
    let snapshot = vm.load_script("math", DEMO_SCRIPT)?;
    let result = vm.call_function(
        "math",
        "sum",
        vec![serde_json::json!({ "left": 20, "right": 22 })],
    )?;

    println!("loaded: {:?}", snapshot);
    println!("result: {result}");
    println!("stats: {:?}", vm.stats()?);
    println!("event: {:?}", subscription.recv());
    println!("event: {:?}", subscription.recv());

    vm.shutdown()?;
    Ok(())
}
