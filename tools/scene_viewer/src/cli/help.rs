pub(crate) fn print_help() {
    println!(
        "scene_viewer\n\n
actions:\n  render   Render a .scenebundle into a grayscale PNG using device-like compositing\n  inspect  Print bundle summary\n\n
action: render\n  --bundle FILE           Input bundle (default: tools/scene_maker/out/scene.scenebundle)\n  --out FILE              Output PNG (default: tools/scene_viewer/out/render.png)\n  --mode MODE             mono1|gray3|gray4|gray8 (default: gray3)\n  --dither MODE           none|bayer4 (default: bayer4)\n  --edge-strength N       0..255 (default: 96)\n  --fog-strength N        0..255 (default: 72)\n  --stroke-strength N     0..255 (default: 24)\n  --tone-curve MODE       linear|wash|filmic (default: wash)\n  --save-debug DIR        Save intermediates (tone base / stylized / quantized)\n  --dump-channels DIR     Save decoded source channels\n  --ghost-from FILE       Prior rendered frame for ghosting simulation\n  --ghost-alpha N         0..255 blend amount from prior frame (default: 0)\n\n
action: inspect\n  --bundle FILE           Bundle path"
    );
}
