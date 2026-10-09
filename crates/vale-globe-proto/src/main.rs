use std::path::PathBuf;

use vale_globe_proto::app::DEFAULT_FACE_SIZE;

const USAGE: &str = "\
vale-globe-proto [--face N] [--screenshot FILE] [--size WxH] [--bench]

  --face N           Texels on one edge of a cube face. The default is 1024.
  --screenshot FILE  Paint a fixed set of strokes and write the UI to a PNG.
  --size WxH         The size of the screenshot in points. The default is 1180x820.
  --bench            Print the time of the brush on the CPU and stop.";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut face = DEFAULT_FACE_SIZE;
    let mut screenshot: Option<PathBuf> = None;
    let mut size = (1180.0_f32, 820.0_f32);
    let mut bench = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value\n\n{USAGE}"));
        match arg.as_str() {
            "--face" => face = value()?.parse()?,
            "--screenshot" => screenshot = Some(value()?.into()),
            "--size" => {
                let v = value()?;
                let (w, h) = v.split_once('x').ok_or("--size needs WxH")?;
                size = (w.parse()?, h.parse()?);
            }
            "--bench" => bench = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => return Err(format!("unknown argument {other}\n\n{USAGE}").into()),
        }
    }
    if !(64..=4096).contains(&face) {
        return Err("--face must be from 64 to 4096".into());
    }
    if bench {
        print!("{}", vale_globe_proto::headless::bench(face));
        return Ok(());
    }
    if let Some(path) = screenshot {
        let image = vale_globe_proto::headless::screenshot(face, size)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        image.save(&path)?;
        println!("wrote {}", path.display());
        return Ok(());
    }
    vale_globe_proto::run(face)?;
    Ok(())
}
