//! Run the production scheduling policy and index tests on the host. Only the
//! product-state input is substituted; priorities and metadata use real code.
#![allow(dead_code)]

mod firmware {
    pub mod app_state {
        #[derive(Clone, Copy, Default)]
        pub enum Phase {
            #[default]
            Initializing,
            DiagnosticsExclusive,
        }

        #[derive(Clone, Copy, Default)]
        pub struct ServiceFlags {
            pub upload_enabled: bool,
        }

        #[derive(Clone, Copy, Default)]
        pub struct AppStateSnapshot {
            pub phase: Phase,
            pub services: ServiceFlags,
        }
    }
}

#[path = "../../../products/meditamer/src/firmware/scheduling.rs"]
mod scheduling;
