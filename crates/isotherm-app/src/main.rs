use clap::Parser;
use isotherm_app::cli::Args;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if let Some(path) = &args.screenshot {
        let image = isotherm_app::headless::screenshot(&args)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        image.save(path)?;
        println!("wrote {}", path.display());
        return Ok(());
    }
    isotherm_app::run(args)?;
    Ok(())
}
