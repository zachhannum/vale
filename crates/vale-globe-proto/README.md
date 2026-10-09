# vale-globe-proto

This crate is a prototype. It tests one design goal of Vale: you paint a heightmap on a globe with a pen, and the brush does not stretch at any place on the sphere. It also answers an open question of the design: is egui good enough for Apple Pencil on iPad?

The heightmap and the brush come from the crate `vale-terrain`. The crate is not published.

## Run on the desktop

```sh
cargo run --release -p vale-globe-proto
```

| Input | Result |
| --- | --- |
| Left drag | Paint, or draw a line |
| Right drag, or Shift and left drag | Rotate the globe |
| Scroll or pinch | Zoom |

A mouse has no pressure, so the flow is fixed at one half.

This command paints a fixed scene and writes the UI to a PNG, with no window:

```sh
cargo run --release -p vale-globe-proto -- --screenshot target/globe-proto/demo.png
```

This command prints the time of one brush stamp on the CPU:

```sh
cargo run --release -p vale-globe-proto -- --bench
```

## Run on an iPad

One script builds the app, installs it on the iPad, and starts it. You do not need to open Xcode.

Do these steps one time, on the Mac:

1. Install the tools:

   ```sh
   brew install xcodegen
   rustup target add aarch64-apple-ios
   xcodebuild -downloadPlatform iOS
   ```

   The last command downloads about 9 GB.

Do these steps one time, on the iPad:

1. Connect the iPad to the Mac with a USB cable, and unlock the iPad.
2. If the iPad asks "Trust This Computer?", tap Trust, and type the passcode of the iPad.
3. Open Settings, then Privacy & Security, then Developer Mode. Turn on Developer Mode.
4. The iPad asks to restart. Tap Restart.
5. After the restart, unlock the iPad, and tap Turn On in the Developer Mode alert.

If Developer Mode is not in the list, run the script below one time. The script fails, and then Developer Mode shows in the list.

Then run the script, with the iPad connected and unlocked:

```sh
scripts/globe-proto-ipad.sh
```

The script prints four steps and then "Done". The app "Vale Globe" is then open on the iPad. The first build takes a few minutes.

If the script stops, it prints the cause and what to do. These are the usual causes:

| Message | What to do |
| --- | --- |
| No iPad found | Make sure that the cable is connected and that the iPad is unlocked. Tap Trust on the iPad. |
| No Apple developer team found | Open the Xcode app. In the menu bar, select Xcode, then Settings, then Accounts. Press the plus button and sign in with your Apple ID. |
| The build failed | Read the lines above the message. The full log is in `target/ios/vale-globe-proto/xcodebuild.log`. |
| The app is installed, but it did not start | On the iPad, open Settings, then General, then VPN & Device Management. Tap your developer name, and tap Trust. Then tap the Vale Globe icon. |

To run the app again later, tap the Vale Globe icon on the iPad. Run the script again only after the code changes.

## What to test on the iPad

The panel "Pen and timing" shows what the app receives from the pen. Do each test, and write down the result.

| Test | How | Good result |
| --- | --- | --- |
| Pressure | Draw one stroke from light to hard. | "Pen force" changes, and the flow graph rises without steps. The stroke goes from faint to strong. |
| Pen and finger | Touch the globe with the pen, then with a finger. | "Touches" counts one pen and one finger. The pen paints, and the finger rotates. |
| Palm | Rest your hand on the screen and paint. | The globe does not move, and "Palms ignored" goes up. |
| Navigation | Rotate with one finger. Pinch and twist with two fingers. | The place under your fingers stays under your fingers. |
| Sample rate | Draw fast circles for two seconds. | "Last stroke" shows the samples per second. Apple Pencil makes 240. |
| Smooth strokes | Draw fast curves with the Line tool and with a small brush. | The curves have no corners. |
| Lag | Select "Draw every frame". Draw fast, and watch the gap from the pen tip to the paint. | The gap is small enough that you can draw a coast. "Frame" stays near 8 ms on a 120 Hz iPad, or 16 ms on a 60 Hz iPad. |
| Large brush | Set the size to 160 pt and the face to 2048, and paint. | "Brush on CPU" stays below the frame time. |
| Hover | Hold the pen tip a few millimeters above the screen and move it. | "Hover" shows "seen", and the brush circle follows the pen. |
| Poles | Press "North pole" and paint dots and strokes over the pole. | The dots are round. Rotate the globe, and they stay round. |
| Panels | Use each slider and button with a finger. | You hit each control at the first try, and the globe does not move under the panel. |

## What the source code predicts

The app uses eframe 0.36.2 and winit 0.30.13. The code of those two crates gives these predictions. The iPad test tells you if they are correct.

- Pressure works. winit reads the force of Apple Pencil, and egui passes it on.
- The app can tell the pen from a finger. egui has no pen type, but on iPad only Apple Pencil has a force, so the app treats a touch with a force as a pen.
- Hover does not work. winit does not ask iOS for hover events.
- The sample rate is the frame rate of the screen, 60 or 120 per second, and not 240. iOS collects the extra pen samples in each frame (coalesced touches), and winit does not read them.
- Tilt does not arrive. winit reads the tilt of the pen, and egui drops it.

If hover or the sample rate fails the test, the next step is a small piece of UIKit code below egui. A gesture recognizer on the view of the app can read hover, coalesced touches, and tilt, and it can send them to the canvas. egui then draws only the panels. This prototype does not have that code yet.

## How the prototype differs from the design

- The brush stamps on the CPU, and the app sends the changed texels to the GPU. The design stamps in a wgpu shader. The time in "Brush on CPU" tells you how much that matters.
- The heightmap has one level of tiles. The GPU texture holds six full faces.
- The app saves nothing. There is no project file.
- The shader reads the cube map with its own bilinear filter and does not blend across a face edge.
- Undo keeps copies of the changed tiles for the last eight strokes.
- There is no polygonize tool. Lines are not stored as features.
