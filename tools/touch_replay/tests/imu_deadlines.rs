#![allow(dead_code)]
mod firmware {
    pub mod types {
        pub mod i2c {
            #[derive(Clone, Copy, Default)]
            pub struct BusTiming {
                pub queue_us: u32,
                pub admission_us: u32,
                pub mutex_us: u32,
                pub config_us: u32,
                pub transfer_us: u32,
                pub attempts: u32,
                pub errors: u32,
            }

            #[derive(Clone, Copy, Default)]
            pub struct WaitDetail {
                pub wait_us: u32,
            }

            impl WaitDetail {
                pub const ZERO: Self = Self { wait_us: 0 };
            }

            #[derive(Clone, Copy, Default)]
            pub struct ImuBusTiming {
                pub stages: [BusTiming; 3],
                pub waits: [WaitDetail; 2],
            }
        }
    }
}

extern crate self as inkplate_tempera;
pub mod imu {
    #[derive(Clone, Copy, Default)]
    pub struct ImuReadTiming {
        pub interrupt_port_us: u32,
        pub sensor_us: u32,
    }
}

#[path = "../../../products/meditamer/src/firmware/imu/scheduler.rs"]
mod scheduler;
#[path = "../../../products/meditamer/src/firmware/imu/timing.rs"]
mod timing;
