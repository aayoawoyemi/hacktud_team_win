//! CONCEPT: dev binary for the bench library: `bench <bench.toml>`.
//! WHY: WP-8 needs a runnable tool now (Checkpoint B numbers). O-02 puts every tool behind one shared `pool` CLI
//!      later; this file then becomes a one-line subcommand hook. Config only (O-24), no flags: one way to run it.
//! DEPENDS ON: bench library; crates/bench/bench.example.toml; prompts/bench.toml.
//! AFFECTS: WP-13 runbook quotes this command.
//!
//! Exit codes: 0 = every measured request succeeded, 1 = some failed (results still written), 2 = bad config.

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [cfg_path] = args.as_slice() else {
        eprintln!("usage: bench <bench.toml>   (example: crates/bench/bench.example.toml)");
        return ExitCode::from(2);
    };
    let setup = bench::load_config(Path::new(cfg_path))
        .and_then(|cfg| bench::load_prompts(&cfg.prompts_file, &cfg.prompt_use).map(|p| (cfg, p)));
    let (cfg, prompts) = match setup {
        Ok(x) => x,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    eprintln!(
        "bench: {} prompt(s) x ({} warm-up + {} runs) against {} [{}]",
        prompts.len(),
        cfg.warmup,
        cfg.repeats,
        cfg.endpoint,
        cfg.scenario
    );
    match bench::run(&cfg, &prompts, |line| eprintln!("  {line}")) {
        Ok((results, md)) => {
            println!("{md}");
            eprintln!("results in {}", cfg.output_dir.display());
            if results.iter().any(|r| !r.warmup && r.error.is_some()) {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}
