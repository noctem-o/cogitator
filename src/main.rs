use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

mod eval;
mod model;

#[cfg(feature = "tui")]
mod tui;

/// CLI arguments
#[derive(Parser, Debug)]
#[command(name = "cogitator", version, about = "Deterministic evaluation harness")]
pub struct Args {
    #[arg(long, default_value_t = 42)]
    pub seed: u64,

    #[arg(long, default_value_t = 5000)]
    pub runs: u32,

    #[arg(long, default_value = "results.csv")]
    pub output: PathBuf,

    #[arg(long)]
    pub no_tui: bool,

    #[arg(long, default_value_t = true)]
    pub parallel: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let results = if args.parallel {
        eval::run_parallel(args.seed, args.runs)
    } else {
        eval::run_sequential(args.seed, args.runs)
    };

    eval::write_results(&args.output, &results)?;
    let summary = eval::summarize(&results);

    if !args.no_tui {
        #[cfg(feature = "tui")]
        tui::launch(args.seed, args.runs, &results, &summary)?;

        #[cfg(not(feature = "tui"))]
        println!("TUI disabled (missing feature).");
    }

    println!(
        "Seed={} Runs={} PassRate={:.2}% AvgScore={:.3} Output={}",
        args.seed,
        args.runs,
        summary.pass_rate * 100.0,
        summary.avg_score,
        args.output.display()
    );

    Ok(())
}


