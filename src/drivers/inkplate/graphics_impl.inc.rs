#[cfg(feature = "graphics")]
impl<I2C, D> OriginDimensions for InkplateHal<I2C, D> {
    fn size(&self) -> Size {
        Size::new(E_INK_WIDTH as u32, E_INK_HEIGHT as u32)
    }
}

#[cfg(feature = "graphics")]
impl<I2C, D> DrawTarget for InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
    type Color = BinaryColor;
    type Error = Infallible;

    fn draw_iter<It>(&mut self, pixels: It) -> core::result::Result<(), Self::Error>
    where
        It: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels {
            if point.x < 0 || point.y < 0 {
                continue;
            }
            self.set_pixel_bw(point.x as usize, point.y as usize, color == BinaryColor::On);
        }
        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> core::result::Result<(), Self::Error> {
        if color == BinaryColor::On {
            self.framebuffer_bw.fill(0xFF);
        } else {
            self.framebuffer_bw.fill(0x00);
        }
        Ok(())
    }
}
