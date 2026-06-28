pub const WIDTH: usize = 128;
pub const HEIGHT: usize = 64;
pub const FRAME_SIZE: usize = 1024;

pub fn set_pixel(frame: &mut [u8; FRAME_SIZE], x: usize, y: usize, on: bool) {
    if x >= WIDTH || y >= HEIGHT {
        return;
    }

    let page = y / 8;
    let bit = y % 8;
    let byte_index = page * WIDTH + x;

    if on {
        frame[byte_index] |= 1u8 << bit;
    } else {
        frame[byte_index] &= !(1u8 << bit);
    }
}

pub fn checkerboard_frame() -> [u8; FRAME_SIZE] {
    let mut frame = [0u8; FRAME_SIZE];

    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let block_x = x / 8;
            let block_y = y / 8;

            if (block_x + block_y) % 2 == 0 {
                set_pixel(&mut frame, x, y, true);
            }
        }
    }

    frame
}
