pub const OLED_WIDTH: usize = 128;
pub const OLED_HEIGHT: usize = 64;
pub const OLED_FRAME_SIZE: usize = 1024;

pub const PACKET_FRAME: u8 = 0x01;
pub const PACKET_CLEAR: u8 = 0x02;
pub const PACKET_PLAY: u8 = 0x03;
pub const PACKET_PAUSE: u8 = 0x04;

pub fn validate_frame(frame: &[u8]) -> bool {
    frame.len() == OLED_FRAME_SIZE
}
