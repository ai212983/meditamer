mod acquisition;
mod pipeline;

use super::{super::types::InkplateDriver, config::TOUCH_FEEDBACK_RADIUS_PX};

pub(crate) use acquisition::touch_acquisition_task;
pub(crate) use pipeline::{
    push_touch_input_sample, request_touch_pipeline_reset, touch_pipeline_task,
};

pub(crate) fn draw_touch_feedback_dot(display: &mut InkplateDriver, x: u16, y: u16) {
    let cx = x as i32;
    let cy = y as i32;
    let radius = TOUCH_FEEDBACK_RADIUS_PX.max(1);
    let radius_sq = radius * radius;
    let width = display.width() as i32;
    let height = display.height() as i32;

    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dy * dy > radius_sq {
                continue;
            }
            let px = cx + dx;
            let py = cy + dy;
            if px >= 0 && py >= 0 && px < width && py < height {
                display.set_pixel_bw(px as usize, py as usize, true);
            }
        }
    }
}
