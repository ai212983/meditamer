//! Command-line surface: the render configuration model, the parser, and help text.

mod help;
mod model;
mod parse;
#[cfg(test)]
mod tests;

pub(crate) use help::print_help;
pub(crate) use model::{Config, DitherMode, OutputMode, ToneCurve};
pub(crate) use parse::{mode_name, next_value, parse_render_args};
