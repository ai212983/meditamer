#[cfg(feature = "asset-upload-http")]
include!("recorders/wifi.rs");
#[cfg(feature = "asset-upload-http")]
include!("recorders/upload_net.rs");
include!("recorders/sd_upload.rs");
include!("recorders/stack.rs");
include!("recorders/diag_mask.rs");
#[cfg(feature = "asset-upload-http")]
include!("recorders/helpers.rs");
