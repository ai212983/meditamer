//! Medinote's own Hourglass app-registration glue: the [`descriptor`] module
//! (provider/surface identity and catalogue metadata) `catalogue::build` and
//! the target's registration code need. Everything else the on-glass
//! Hourglass app is built from -- the physics model, presentation, and
//! rendering geometry -- moved to the chip-neutral `hourglass` workspace
//! crate, since none of it named a product or hardware type; this module is
//! what remains local to Medinote.

pub mod descriptor;
