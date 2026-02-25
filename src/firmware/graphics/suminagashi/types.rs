#[derive(Clone, Copy)]
pub enum RgssMode {
    X1,
    X4,
    X8,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    Mono1,
    Gray4,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DitherMode {
    Bayer4x4,
    BlueNoise32,
    BlueNoise600,
}

#[derive(Clone, Copy)]
pub struct SceneRenderStyle {
    pub rgss: RgssMode,
    pub mode: RenderMode,
    pub dither: DitherMode,
}
