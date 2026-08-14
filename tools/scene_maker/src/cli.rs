//! Command-line surface: the argument model, the parser, and the help text.

mod help;
mod model;
mod parse;
#[cfg(test)]
mod tests;

pub(crate) use help::print_help;
pub(crate) use model::{
    BuildConfig, ChannelId, ChannelTemplate, Compression, ExplicitChannelPaths, CHANNELS,
};
pub(crate) use parse::{next_value, parse_build_args};
