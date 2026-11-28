use std::path::PathBuf;

use eframe::egui;

const USAGE: &str = "\
isotherm-app [--screenshot FILE] [--size WxH] [--layout pad] [--safe-area T,R,B,L]

  --screenshot FILE    Write the UI to a PNG and stop.
  --size WxH           The size of the screenshot in points. The default is 1440x900.
  --layout pad         The iPad layout. It is the only layout now.
  --safe-area T,R,B,L  The safe area of the screenshot: the bands at the top,
                       the right, the bottom, and the left, in points. An
                       iPhone 16 in portrait has 59,0,34,0.";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut screenshot: Option<PathBuf> = None;
    let mut size = (1440.0_f32, 900.0_f32);
    let mut safe_area = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value\n\n{USAGE}"));
        match arg.as_str() {
            "--screenshot" => screenshot = Some(value()?.into()),
            "--size" => {
                let v = value()?;
                let (w, h) = v.split_once('x').ok_or("--size needs WxH")?;
                size = (w.parse()?, h.parse()?);
            }
            "--layout" => {
                let v = value()?;
                if v != "pad" {
                    return Err(format!("unknown layout {v}\n\n{USAGE}").into());
                }
            }
            "--safe-area" => safe_area = Some(parse_safe_area(&value()?)?),
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => return Err(format!("unknown argument {other}\n\n{USAGE}").into()),
        }
    }
    if let Some(path) = screenshot {
        let image = isotherm_app::headless::screenshot(size, safe_area)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        image.save(&path)?;
        println!("wrote {}", path.display());
        return Ok(());
    }
    isotherm_app::run()?;
    Ok(())
}

/// The insets of `--safe-area`: the top, the right, the bottom, and the left.
fn parse_safe_area(text: &str) -> Result<egui::SafeAreaInsets, String> {
    let bands: Vec<f32> = text
        .split(',')
        .map(|b| b.trim().parse::<f32>().map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    let &[top, right, bottom, left] = bands.as_slice() else {
        return Err(format!("--safe-area needs four bands, got {text}"));
    };
    if bands.iter().any(|b| !(b.is_finite() && *b >= 0.0)) {
        return Err(format!(
            "each band of the safe area must be 0 or more, got {text}"
        ));
    }
    Ok(egui::SafeAreaInsets(egui::epaint::MarginF32 {
        left,
        right,
        top,
        bottom,
    }))
}
