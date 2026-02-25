use core::sync::atomic::{AtomicU8, Ordering};

use crate::firmware::types::RuntimeServices;

static RUNTIME_SERVICES: AtomicU8 = AtomicU8::new(RuntimeServices::normal().as_persisted());

pub(crate) fn runtime_services() -> RuntimeServices {
    RuntimeServices::from_persisted(RUNTIME_SERVICES.load(Ordering::Relaxed))
}

pub(crate) fn set_runtime_services(services: RuntimeServices) {
    RUNTIME_SERVICES.store(services.as_persisted(), Ordering::Relaxed);
}

pub(crate) fn upload_enabled() -> bool {
    runtime_services().upload_enabled_flag()
}

pub(crate) fn asset_reads_enabled() -> bool {
    runtime_services().asset_reads_enabled_flag()
}
