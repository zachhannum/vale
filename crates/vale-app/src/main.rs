use std::process::ExitCode;

use anyhow::Context;
use clap::Parser;
use vale_app::app::run_window;
use vale_app::cli::{
    Args, apply_globe, apply_import, apply_measure, apply_view, export_heightmap, parse_pair,
};
use vale_app::globe::{FACE_SIZE, WINDOW_FACE_SIZE};
use vale_app::headless;
use vale_app::pipeline::Pipeline;
use vale_app::session::Session;
use vale_app::ui::raster_form::RasterForm;
use vale_app::ui::world_form::WorldForm;
use vale_app::ui::{Action, AppState};

fn run() -> anyhow::Result<u8> {
    let args = Args::parse();
    if let Some(done) = export_heightmap(&args)? {
        let path = done.path.display();
        println!("wrote {path} ({} x {})", done.width, done.height);
        return Ok(0);
    }
    if (args.export_atlas.is_some() || !args.export_page.is_empty())
        && let Some(path) = &args.project
        && !path.is_file()
    {
        anyhow::bail!("the project file {} does not exist", path.display());
    }
    let mut doc = args.document()?;
    let session = match &args.project {
        Some(path) => {
            Some(Session::open(path, &mut doc).with_context(|| format!("{}", path.display()))?)
        }
        None => None,
    };
    let page_export = args.page_export()?;
    let size = args.size()?;
    let mut pipeline = Pipeline::new();
    apply_view(&args, &mut doc, &mut pipeline, size)?;
    let ratio = args.pixel_ratio;

    let headless_run = args.export.is_some()
        || args.export_atlas.is_some()
        || page_export.is_some()
        || args.screenshot.is_some()
        || !args.probe.is_empty()
        || args.report
        || args.stroke_test
        || args.zoom_test;
    let new_state = |doc, face_size| -> anyhow::Result<AppState> {
        let mut state = AppState::new(doc)?;
        // A new world gets a file next to the project file.
        state.projects = args.locations(!headless_run);
        state.recover_projects();
        if args.starts_on_projects(!headless_run) {
            state.show_projects();
        }
        state.workspace = args.workspace;
        state.set_layout(args.layout);
        for panel in &args.panel {
            state.pad.open(*panel);
        }
        state.pad.modes = args.brush_modes;
        apply_measure(&args, &mut state)?;
        let world = &state.doc.project.world;
        state.world_form = args.world_form.map(|mode| WorldForm::open(mode, world));
        if args.worlds_menu {
            vale_app::ui::pad::toggle_worlds(&mut state);
        }
        apply_globe(&args, &mut state.globe, face_size)?;
        state.form_face_size = args.face_size.map(|size| size as usize);
        if args.raster_form {
            state.raster_form = Some(RasterForm::open(&state));
        }
        if let Some(session) = &session {
            state.open_rasters(session.load_display_maps()?);
        }
        Ok(state)
    };
    if !headless_run {
        let mut state = new_state(doc, WINDOW_FACE_SIZE)?;
        apply_import(&args, &mut state, false)?;
        // The start screen opens only if the last run ended there.
        let restore = args.starts_on_projects(true) && !args.start_screen;
        let frames = args.smoke_frames.map(u64::from);
        run_window(state, size, frames, session, restore)?;
        return Ok(0);
    }
    if args.export_atlas.is_some() || page_export.is_some() {
        let mut state = new_state(doc.clone(), FACE_SIZE)?;
        state.headless = false;
        let action = match (&args.export_atlas, &page_export) {
            (Some(path), _) => Action::ExportAtlas(path.clone()),
            (None, Some((number, path))) => {
                let page = state
                    .doc
                    .project
                    .atlas
                    .pages
                    .get(number - 1)
                    .with_context(|| {
                        format!(
                            "page {number} is outside the atlas of {} pages",
                            state.doc.project.atlas.pages.len()
                        )
                    })?;
                state.atlas.page = page.id;
                Action::Export(path.clone())
            }
            (None, None) => unreachable!(),
        };
        let setup = egui_kittest::wgpu::default_wgpu_setup();
        let mut harness = headless::ui_harness(state, (128.0, 128.0), 1.0, setup);
        harness.set_render_every_step(true);
        harness.run_steps(3);
        harness.state_mut().run_action(action);
        let limit = harness.state().doc.project.atlas.pages.len() * 20 + 20;
        for _ in 0..limit {
            harness.run_steps(1);
            if harness.state().atlas_export.is_none() {
                break;
            }
        }
        if harness.state().atlas_export.is_some() {
            anyhow::bail!("the atlas export did not end");
        }
        let status = &harness.state().status;
        if !status.starts_with("Exported ") {
            anyhow::bail!("{status}");
        }
        let path = args
            .export_atlas
            .as_ref()
            .or_else(|| page_export.as_ref().map(|(_, path)| path))
            .unwrap();
        println!("wrote {}", path.display());
        return Ok(0);
    }
    if args.stroke_test {
        let state = new_state(doc.clone(), FACE_SIZE)?;
        print!("{}", headless::stroke_test(state, size, ratio)?);
    }
    if args.zoom_test {
        let state = new_state(doc.clone(), FACE_SIZE)?;
        println!("{}", headless::zoom_test(state, size, ratio)?);
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
            // The renderer can make the face size smaller, and that drops an
            // import. So the import runs after the renderer is there.
            let import = |state: &mut AppState| {
                apply_import(&args, state, true)?;
                // The flyout of the brush modes closes while no layer is
                // painted, so it opens after the import.
                state.pad.modes |= args.brush_modes;
                Ok(())
            };
            let safe_area = args.safe_area()?;
            let (image, state) = headless::ui_png_safe(state, size, ratio, safe_area, import)?;
            if args.import_heightmap.is_some() || args.import_ramp.is_some() {
                println!("{}", state.status);
            }
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
