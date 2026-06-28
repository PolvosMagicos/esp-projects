use anyhow::{anyhow, Result};
use embedded_graphics::{pixelcolor::BinaryColor, prelude::*, Pixel};

use crate::frame::{FRAME_SIZE, HEIGHT, WIDTH};

pub fn draw_raw_frame<D>(display: &mut D, frame: &[u8; FRAME_SIZE]) -> Result<()>
where
    D: DrawTarget<Color = BinaryColor>,
    D::Error: core::fmt::Debug,
{
    let pixels = (0..HEIGHT).flat_map(|y| {
        (0..WIDTH).map(move |x| {
            let page = y / 8;
            let bit = y % 8;
            let byte_index = page * WIDTH + x;

            let is_on = (frame[byte_index] & (1u8 << bit)) != 0;

            Pixel(
                Point::new(x as i32, y as i32),
                if is_on {
                    BinaryColor::On
                } else {
                    BinaryColor::Off
                },
            )
        })
    });

    display
        .draw_iter(pixels)
        .map_err(|err| anyhow!("OLED draw error: {:?}", err))?;

    Ok(())
}
