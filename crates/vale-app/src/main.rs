use std::process::ExitCode;

use clap::Parser;
use vale_app::app::run_window;
use vale_app::cli::{Args, apply_globe, apply_view, parse_pair};
use vale_app::globe::{FACE_SIZE, WINDOW_FACE_SIZE};
use vale_app::headless;
use vale_app::pipeline::Pipeline;
use vale_app::ui::AppState;

fn run() -> anyhow::Result<u8> {
    let args = Args::parse();
    let mut doc = args.document()?;
    let size = args.size()?;
    let mut pipeline = Pipeline::new();
    apply_view(&args, &mut doc, &mut pipeline, size)?;
    let ratio = args.pixel_ratio;

    let headless_run = args.export.is_some()
        || args.screenshot.is_some()
        || !args.probe.is_empty()
        || args.report
        || args.stroke_test;
    let new_state = |doc, face_size| -> anyhow::Result<AppState> {
        let mut state = AppState::new(doc)?;
        state.workspace = args.workspace;
        state.set_layout(args.layout);
        for panel in &args.panel {
            state.pad.open(*panel);
        }
        apply_globe(&args, &mut state.globe, face_size)?;
        Ok(state)
    };
    if !headless_run {
        let state = new_state(doc, WINDOW_FACE_SIZE)?;
        run_window(state, size, args.smoke_frames.map(u64::from))?;
        return Ok(0);
    }
    if args.stroke_test {
        let state = new_state(doc.clone(), FACE_SIZE)?;
        print!("{}", headless::stroke_test(state, size, ratio)?);
    }

    let mut composed = None;
    if let Some(path) = &args.export {
        composed = Some(headless::export(&doc, size, ratio, path)?);
        println!("wrote {}", path.display());
    }
    if let Some(path) = &args.screenshot {
        if args.map_only {
            let image = headless::map_png(&doc, size, ratio)?;
            headless::write_file(path, &image.png)?;
            println!(
                "wrote {} ({} x {})",
                path.display(),
                image.width,
                image.height
            );
            composed = Some(image.composed);
        } else {
            let state = new_state(doc.clone(), FACE_SIZE)?;
            let (image, state) = headless::ui_png(state, size, ratio)?;
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
            {
                std::fs::create_dir_all(parent)?;
            }
            image.save(path)?;
            println!(
                "wrote {} ({} x {})",
                path.display(),
                image.width(),
                image.height()
            );
            composed = state.composed;
        }
    }
    for spec in &args.probe {
        let at = parse_pair(spec)?;
        match headless::probe(&doc, size, ratio, at)? {
            Some((pos, [r, g, b])) => println!(
                "probe {},{} -> page {:.1},{:.1} rgb {r},{g},{b}",
                at[0], at[1], pos.x, pos.y
            ),
            None => println!("probe {},{} -> not visible", at[0], at[1]),
        }
    }
    if args.report {
        let composed = match composed {
            Some(c) => c,
            None => headless::map_png(&doc, size, ratio)?.composed,
        };
        print!("{}", headless::report(&doc, &composed));
    }
    Ok(0)
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(1)
        }
    }
}
