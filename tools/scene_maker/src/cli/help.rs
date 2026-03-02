pub(crate) fn print_help() {
    println!(
        "scene_maker\n\n
actions:\n  build   Pack pre-baked map images into a strip-major .scenebundle\n  inspect Inspect bundle metadata/compression summary\n\n
action: build\n  --input DIR            Input directory (default: tools/scene_maker/input)\n  --out FILE             Output bundle path (default: tools/scene_maker/out/scene.scenebundle)\n  --metadata FILE        Output metadata json path\n  --width N              Target width (default: 600)\n  --height N             Target height (default: 600)\n  --strip-height N       Strip height in rows (default: 32)\n  --compression MODE     none|rle (default: rle)\n  --derive-edge BOOL     true|false (default: true)\n  --albedo FILE          Override albedo map path\n  --light FILE           Override light map path\n  --ao FILE              Override ao map path\n  --depth FILE           Override depth map path\n  --edge FILE            Override edge map path\n  --mask FILE            Override mask map path\n  --stroke FILE          Override stroke map path\n  --normal-x FILE        Override normal_x map path\n  --normal-y FILE        Override normal_y map path\n\n  If overrides are not set, files are discovered in --input using names:\n  albedo/light/ao/depth/edge/mask/stroke/normal_x/normal_y + extension .png\n\n
action: inspect\n  --bundle FILE          Bundle to inspect (default: tools/scene_maker/out/scene.scenebundle)"
    );
}
