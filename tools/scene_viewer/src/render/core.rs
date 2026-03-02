use std::collections::HashMap;

use crate::cli::{parse_render_args, DitherMode, OutputMode, ToneCurve};

mod flow;

const CH_ALBEDO: u8 = 1;
const CH_LIGHT: u8 = 2;
const CH_AO: u8 = 3;
const CH_DEPTH: u8 = 4;
const CH_EDGE: u8 = 5;
const CH_MASK: u8 = 6;
const CH_STROKE: u8 = 7;
const CH_NORMAL_X: u8 = 8;
const CH_NORMAL_Y: u8 = 9;

pub(crate) fn run_render<I>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = String>,
{
    let cfg = parse_render_args(args)?;
    flow::run_render_with_config(cfg)
}

