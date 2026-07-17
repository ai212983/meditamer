use super::*;
use crate::drivers::inkplate::{
    TouchInitStatus, TouchPoint, TouchSample, E_INK_HEIGHT, E_INK_WIDTH, TOUCHSCREEN_ADDR,
    TOUCH_GET_X_RES_CMD, TOUCH_GET_Y_RES_CMD, TOUCH_HELLO_PACKET, TOUCH_SOFT_RESET_CMD,
    TOUCH_SOFT_RESET_POLL_INTERVAL_MS, TOUCH_SOFT_RESET_TIMEOUT_MS,
};

const RAW_EMPTY_RETRY_COUNT: u8 = 2;
const RAW_EMPTY_RETRY_DELAY_MS: u64 = 1;

impl<I2C> InkplateTouch<I2C>
where
    I2C: I2cOps,
{
    async fn software_reset_read_hello(&mut self) -> Result<[u8; 4], I2C::Error> {
        self.i2c_write(TOUCHSCREEN_ADDR, &TOUCH_SOFT_RESET_CMD)
            .await?;
        let mut hello = [0u8; 4];
        let attempts = TOUCH_SOFT_RESET_TIMEOUT_MS
            .saturating_div(TOUCH_SOFT_RESET_POLL_INTERVAL_MS)
            .max(1);
        for _ in 0..attempts {
            embassy_time::Timer::after_millis(TOUCH_SOFT_RESET_POLL_INTERVAL_MS as u64).await;
            self.i2c_read(TOUCHSCREEN_ADDR, &mut hello).await?;
            if hello == TOUCH_HELLO_PACKET {
                break;
            }
        }
        Ok(hello)
    }

    async fn set_power_state(&mut self, powered: bool) -> Result<(), I2C::Error> {
        let mut cmd = [0x54, 0x50, 0x00, 0x01];
        if powered {
            cmd[1] |= 1 << 3;
        }
        self.i2c_write(TOUCHSCREEN_ADDR, &cmd).await
    }

    async fn read_resolution(&mut self) -> Result<(u16, u16), I2C::Error> {
        let mut response = [0u8; 4];
        self.i2c_write(TOUCHSCREEN_ADDR, &TOUCH_GET_X_RES_CMD)
            .await?;
        self.i2c_read(TOUCHSCREEN_ADDR, &mut response).await?;
        let x_res = u16::from(response[2]) | (u16::from(response[3] & 0xF0) << 4);
        self.i2c_write(TOUCHSCREEN_ADDR, &TOUCH_GET_Y_RES_CMD)
            .await?;
        self.i2c_read(TOUCHSCREEN_ADDR, &mut response).await?;
        let y_res = u16::from(response[2]) | (u16::from(response[3] & 0xF0) << 4);
        Ok((x_res, y_res))
    }

    pub async fn init_with_status(&mut self) -> Result<TouchInitStatus, I2C::Error> {
        self.set_power_enabled(true).await?;
        embassy_time::Timer::after_millis(180).await;
        self.hardware_reset().await?;

        let mut last_hello = [0u8; 4];
        let mut last_res = (0u16, 0u16);
        for attempt in 0..3u64 {
            let hello = self.software_reset_read_hello().await?;
            last_hello = hello;
            if hello != TOUCH_HELLO_PACKET {
                if attempt < 2 {
                    let _ = self.hardware_reset().await;
                    embassy_time::Timer::after_millis(15 + attempt * 10).await;
                    continue;
                }
                return Ok(TouchInitStatus::HelloMismatch { hello });
            }
            let (x_res, y_res) = self.read_resolution().await?;
            last_res = (x_res, y_res);
            if x_res == 0 || y_res == 0 {
                if attempt < 2 {
                    embassy_time::Timer::after_millis(10 + attempt * 10).await;
                    continue;
                }
                return Ok(TouchInitStatus::ZeroResolution { x_res, y_res });
            }
            self.touch_x_res = x_res;
            self.touch_y_res = y_res;
            self.set_power_state(true).await?;
            return Ok(TouchInitStatus::Ready { x_res, y_res });
        }

        if last_hello != TOUCH_HELLO_PACKET {
            Ok(TouchInitStatus::HelloMismatch { hello: last_hello })
        } else {
            Ok(TouchInitStatus::ZeroResolution {
                x_res: last_res.0,
                y_res: last_res.1,
            })
        }
    }

    pub async fn shutdown(&mut self) -> Result<(), I2C::Error> {
        let _ = self.set_power_state(false).await;
        self.set_power_enabled(false).await
    }

    async fn read_raw_data(&mut self) -> Result<[u8; 8], I2C::Error> {
        let mut raw = [0u8; 8];
        self.i2c_read(TOUCHSCREEN_ADDR, &mut raw).await?;
        Ok(raw)
    }

    pub async fn read_sample(&mut self, rotation: u8) -> Result<TouchSample, I2C::Error> {
        if self.touch_x_res == 0 || self.touch_y_res == 0 {
            let (x_res, y_res) = self.read_resolution().await?;
            self.touch_x_res = x_res;
            self.touch_y_res = y_res;
        }
        let mut raw = self.read_raw_data().await?;
        for _ in 0..RAW_EMPTY_RETRY_COUNT {
            if raw_frame_has_contact(&raw, self.touch_x_res, self.touch_y_res) {
                break;
            }
            embassy_time::Timer::after_millis(RAW_EMPTY_RETRY_DELAY_MS).await;
            raw = self.read_raw_data().await?;
        }
        let bit_count = (raw[7].count_ones() as u8).min(2);
        let mut raw_points = [(0u16, 0u16); 2];
        for (idx, slot) in raw_points.iter_mut().enumerate() {
            let decoded = decode_xy(&raw, idx);
            *slot = if raw_point_plausible(decoded.0, decoded.1, self.touch_x_res, self.touch_y_res)
            {
                decoded
            } else {
                (0, 0)
            };
        }
        if raw_points[0] == (0, 0) && raw_points[1] != (0, 0) {
            raw_points.swap(0, 1);
        }
        let coord_count = raw_points
            .iter()
            .filter(|(x, y)| *x != 0 || *y != 0)
            .count() as u8;
        let touch_count = bit_count.max(coord_count).min(2);
        let points = raw_points.map(|(x, y)| {
            if x == 0 && y == 0 {
                TouchPoint::default()
            } else {
                transform_point(x, y, rotation, self.touch_x_res, self.touch_y_res)
            }
        });
        Ok(TouchSample {
            touch_count,
            points,
            raw,
        })
    }
}

pub(super) fn decode_xy(raw: &[u8; 8], index: usize) -> (u16, u16) {
    let base = 1 + index * 3;
    let d0 = raw[base];
    (
        (u16::from(d0 & 0xF0) << 4) | u16::from(raw[base + 1]),
        (u16::from(d0 & 0x0F) << 8) | u16::from(raw[base + 2]),
    )
}

pub(super) fn raw_point_plausible(x: u16, y: u16, x_res: u16, y_res: u16) -> bool {
    x != 0 && y != 0 && x_res != 0 && y_res != 0 && x <= x_res && y <= y_res
}

pub(super) fn raw_frame_has_contact(raw: &[u8; 8], x_res: u16, y_res: u16) -> bool {
    raw[7].count_ones() > 0
        || (0..2).any(|idx| {
            let (x, y) = decode_xy(raw, idx);
            raw_point_plausible(x, y, x_res, y_res)
        })
}

fn scale_axis(raw: u16, panel_extent: usize, controller_extent: u16) -> u16 {
    if panel_extent == 0 || controller_extent == 0 {
        return 0;
    }
    let panel = panel_extent as u32;
    (u32::from(raw).saturating_mul(panel).saturating_sub(1) / u32::from(controller_extent))
        .min(panel.saturating_sub(1)) as u16
}

pub(super) fn transform_point(
    x_raw: u16,
    y_raw: u16,
    rotation: u8,
    x_res: u16,
    y_res: u16,
) -> TouchPoint {
    let sx = scale_axis(x_raw, E_INK_HEIGHT, x_res);
    let sy = scale_axis(y_raw, E_INK_WIDTH, y_res);
    let max_x = (E_INK_WIDTH - 1) as u16;
    let max_y = (E_INK_HEIGHT - 1) as u16;
    match rotation & 0x03 {
        0 => TouchPoint {
            x: sy,
            y: max_y.saturating_sub(sx),
        },
        1 => TouchPoint {
            x: max_y.saturating_sub(sx),
            y: max_x.saturating_sub(sy),
        },
        2 => TouchPoint {
            x: max_x.saturating_sub(sy),
            y: sx,
        },
        _ => TouchPoint { x: sx, y: sy },
    }
}
