//! Upload mirror: real fence, splitter, helpers, and Abort handler.

#[path = "../../../../../products/meditamer/src/firmware/storage/sd_task/upload/fence.rs"]
mod fence;
#[allow(dead_code)]
#[path = "../../../../../products/meditamer/src/firmware/storage/sd_task/upload/helpers.rs"]
mod helpers;
#[allow(dead_code)]
#[path = "../../../../../products/meditamer/src/firmware/storage/sd_task/upload/types.rs"]
mod types;

use fence::{no_session_abort_fence_result, route_upload_request, UploadRequestRoute};
use types::{split_upload_command, SdUploadSession, UploadCommandGroup, UploadStreamCommand};

#[path = "stream.rs"]
pub mod stream;

#[path = "fence_tests.rs"]
mod fence_tests;
