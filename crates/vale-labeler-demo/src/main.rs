use std::collections::BTreeMap;
use std::path::PathBuf;

use clap::Parser;
use vale_labeler::place_labels;
use vale_labeler_demo::render::{RenderOptions, render, write_png};
use vale_labeler_demo::scene::{Preset, build, fonts};

/// Label Natural Earth data and write a PNG.
#[derive(Parser)]
#[command(name = "vale-labeler-demo", version)]
struct Args {
    #[arg(long, value_enum, default_value = "world")]
    preset: Preset,
    #[arg(long, default_value = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/natural-earth"))]
    data_dir: PathBuf,
    /// Output file. Default is target/demo/<preset>.png
    #[arg(long)]
    out: Option<PathBuf>,
    #[arg(long, default_value_t = 24301)]
    seed: u64,
    #[arg(long)]
    no_improve: bool,
    #[arg(long)]
    debug_boxes: bool,
    /// List every unplaced label with its reason.
    #[arg(long)]
    report: bool,
}

fn run() -> anyhow::Result<()> {
    let args = Args::parse();
    let scene = build(args.preset, &args.data_dir, args.seed, !args.no_improve)?;
    let mut fonts = fonts()?;
    let labeling = place_labels(&mut fonts, &scene.input);

    let line_labels = labeling.placed.iter().filter(|l| l.class == 1).count();
    let point_labels = labeling.placed.len() - line_labels;
    println!(
        "preset {}: {} features, {} placed ({} point, {} line), {} unplaced",
        args.preset.name(),
        scene.input.features.len(),
        labeling.placed.len(),
        point_labels,
        line_labels,
        labeling.unplaced.len()
    );
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    for u in &labeling.unplaced {
        *reasons.entry(format!("{:?}", u.reason)).or_default() += 1;
    }
    if !reasons.is_empty() {
        let list: Vec<String> = reasons.iter().map(|(k, v)| format!("{k} {v}")).collect();
        println!("unplaced by reason: {}", list.join(", "));
    }
    if args.report {
        for u in &labeling.unplaced {
            println!(
                "unplaced: feature {} repeat {} {:?} ({:?})",
                u.feature, u.repeat, u.text, u.reason
            );
        }
    }

    let out = args
        .out
        .unwrap_or_else(|| PathBuf::from(format!("target/demo/{}.png", args.preset.name())));
    let pixmap = render(
        &scene,
        &labeling,
        &RenderOptions {
            debug_boxes: args.debug_boxes,
        },
    );
    write_png(pixmap, &out)?;
    println!(
        "wrote {} ({} x {})",
        out.display(),
        scene.width,
        scene.height
    );
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
