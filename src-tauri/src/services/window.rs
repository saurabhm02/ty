//! Photographs the main screen without one window in it, by asking macOS for only the windows
//! that sit below that window. The window itself never has to hide.
use core_graphics::display::CGDisplay;
use core_graphics::window::{
    create_image, kCGWindowImageDefault, kCGWindowListOptionOnScreenBelowWindow,
};

const BYTES_PER_PIXEL: usize = 4;

/// Photographs the main screen without the window numbered `window_id` or anything above it.
/// Returns the width, the height and the pixels as RGBA bytes.
/// Call it on the main thread: macOS can freeze the app when it is called anywhere else.
pub fn capture_below_chotu(window_id: u32) -> Result<(u32, u32, Vec<u8>), String> {
    click_photograph(kCGWindowListOptionOnScreenBelowWindow, window_id)
}

fn click_photograph(list_option: u32, window_id: u32) -> Result<(u32, u32, Vec<u8>), String> {
    let screen = CGDisplay::main().bounds();
    let img = create_image(screen, list_option, window_id, kCGWindowImageDefault)
        .ok_or("macOS gave no screenshot (is Screen Recording allowed?)")?;

    let (width, height) = (img.width(), img.height());
    if width == 0 || height == 0 {
        return Err("the screenshot is empty".to_string());
    }

    let data = img.data();
    let pxls = rows_to_rgba(data.bytes(), width, height, img.bytes_per_row());
    Ok((width as u32, height as u32, pxls))
}

/// Copies the pixels row by row into one tight list of RGBA bytes.
/// macOS may add spare bytes at the end of each row (`bytes_per_row` is then bigger than
/// `width * 4`), so the spare bytes are skipped. It also swaps blue and red in every pixel,
/// because macOS stores blue-green-red-alpha and the PNG saver wants red-green-blue-alpha.
fn rows_to_rgba(bytes: &[u8], width: usize, height: usize, byte_per_row: usize) -> Vec<u8> {
    let row_len = width * BYTES_PER_PIXEL;
    let mut pxls = Vec::with_capacity(height * row_len);
    for row in 0..height {
        let start = row * byte_per_row;
        pxls.extend_from_slice(&bytes[start..start + row_len]);
    }
    let (pxl_list, _) = pxls.as_chunks_mut::<BYTES_PER_PIXEL>();
    for pixel in pxl_list {
        pixel.swap(0, 2);
    }
    pxls
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_graphics::window::kCGWindowListOptionOnScreenOnly;

    #[test]
    fn blue_and_red_swap_places_in_every_pixel() {
        let one_row = [10, 20, 30, 255, 1, 2, 3, 4];
        assert_eq!(
            rows_to_rgba(&one_row, 2, 1, 8),
            vec![30, 20, 10, 255, 3, 2, 1, 4]
        );
    }

    #[test]
    fn spare_bytes_at_the_end_of_a_row_are_skipped() {
        // 1 pixel per row, but each row is 6 bytes long: 4 pixel bytes and 2 spare ones.
        let two_rows = [1, 2, 3, 4, 99, 99, 5, 6, 7, 8, 99, 99];
        assert_eq!(
            rows_to_rgba(&two_rows, 1, 2, 6),
            vec![3, 2, 1, 4, 7, 6, 5, 8]
        );
    }

    // Needs Screen Recording permission for the terminal, so it is not part of a normal run:
    // cargo test photographs_the_screen -- --ignored
    #[test]
    #[ignore]
    fn photographs_the_screen() {
        let (width, height, pixels) = click_photograph(kCGWindowListOptionOnScreenOnly, 0).unwrap();
        assert!(width > 0 && height > 0);
        assert_eq!(
            pixels.len(),
            width as usize * height as usize * BYTES_PER_PIXEL
        );
    }
}
