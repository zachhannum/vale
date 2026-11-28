use clap::Parser;
use isotherm_app::app::IsothermApp;
use isotherm_app::cli::Args;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let mut app = None;
    if let Some(years) = args.run_years {
        let mut ran = IsothermApp::from_args(&args, None)?;
        println!("{}", ran.run_years(years, args.grid)?);
        app = Some(ran);
    }
    if let Some(path) = &args.screenshot {
        let image = isotherm_app::headless::screenshot(&args, app)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        image.save(path)?;
        println!("wrote {}", path.display());
        return Ok(());
    }
    if app.is_some() {
        return Ok(());
    }
    isotherm_app::run(args)?;
    Ok(())
}
