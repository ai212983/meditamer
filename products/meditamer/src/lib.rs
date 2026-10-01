//! Meditamer product library root.
//!
//! `firmware` moved here from the root package's `src/firmware` in the
//! product and target axis completion plan's Phase 3 -- meditation state,
//! scheduling policy, screens, catalogue, settings, and product assets.
//! `firmware::system`/`firmware::system::tasks` (chip startup and Embassy
//! task wiring) did not move with it; those live in
//! `targets/meditamer-inkplate`, which is also this crate's only consumer.
//! Hardware behavior stays in `boards/inkplate-tempera`, which this crate
//! depends on the same way `targets/meditamer-inkplate` depends on both.

#![no_std]

// The UART console moved to the `console` platform crate (ADR-0015 step 1).
// The old root crate used to alias itself as `esp_println` so that
// `esp_println::println!` resolved there rather than to the upstream crate;
// call sites now say `console::println!`. These two re-exports keep the
// non-macro helpers reachable at the `crate::` paths their callers (moved
// here unchanged in Phase 3) already use.
pub(crate) use console::{dropped_write_count, write_response as write_uart_response};

pub mod firmware;

/// The product name `targets/meditamer-inkplate` composes against.
pub const fn product_name() -> &'static str {
    "meditamer"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_name_identifies_meditamer() {
        assert_eq!(product_name(), "meditamer");
    }
}
