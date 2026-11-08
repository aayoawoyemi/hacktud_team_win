// CONCEPT: WP-5 development inspect/fit utility; shared pool CLI hooks follow O-02.
// WHY: R5.15 makes capacity available to the model owner without a coordinator.
// DEPENDS ON: Local GGUF and a JSON array of capacity::Machine records.
// AFFECTS: inspect/fit/plan dev commands; shared pool CLI hooks stay with WP-12.
use planner::{
    Result,
    capacity::{Machine, fit},
    gguf::read_model,
    plan::{PlanRequest, build_plan},
    shared::model_info,
};
fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("inspect") if args.len()==3 => println!("{}",serde_json::to_string_pretty(&read_model(&args[2])?)?),
        Some("inspect-shared") if args.len()==3 => println!("{}",serde_json::to_string_pretty(&model_info(&read_model(&args[2])?)?)?),
        Some("plan") if args.len()==4 => {
            let model=read_model(&args[2])?;
            let request:PlanRequest=serde_json::from_reader(std::fs::File::open(&args[3])?)?;
            println!("{}",serde_json::to_string_pretty(&build_plan(&model,&request)?)?);
        },
        Some("fit") if (5..=7).contains(&args.len()) => {
            let model=read_model(&args[2])?;
            let machines:Vec<Machine>=serde_json::from_reader(std::fs::File::open(&args[3])?)?;
            let ctx=args[4].parse()?;
            let parallel=args.get(5).map(|v|v.parse()).transpose()?.unwrap_or(1);
            let override_kv=args.get(6).map(|v|v.parse()).transpose()?;
            print!("{}",fit(&model,&machines,ctx,parallel,override_kv)?.table());
        },
        _=>return Err("Usage: planner inspect MODEL.gguf | planner inspect-shared MODEL.gguf | planner plan MODEL.gguf request.json | planner fit MODEL.gguf machines.json CTX [PARALLEL [KV_BYTES_PER_TOKEN]]".into()),
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("planner: {error}");
        std::process::exit(1);
    }
}
