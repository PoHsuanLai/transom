use transom::capture::PixelFormat;
use transom::screencast::pixels::{Layout, convert};

fn layout(format: PixelFormat, stride: usize) -> Layout {
    Layout { format, stride }
}

#[test]
fn the_same_layout_is_copied_row_by_row_with_the_destination_stride() {
    // Two pixels wide, two rows; the source has 4 bytes of padding per row.
    let src = [
        1, 2, 3, 4, 5, 6, 7, 8, 0, 0, 0, 0, //
        9, 10, 11, 12, 13, 14, 15, 16, 0, 0, 0, 0,
    ];
    let mut dst = [0_u8; 16];
    let rows = convert(
        &src,
        layout(PixelFormat::Xrgb8888, 12),
        &mut dst,
        layout(PixelFormat::Xrgb8888, 8),
        2,
        2,
    );
    assert_eq!(rows, 2);
    assert_eq!(dst, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
}

#[test]
fn red_and_blue_swap_between_the_two_byte_orders() {
    // Xrgb8888 is the bytes blue, green, red, padding.
    let src = [10, 20, 30, 0];
    let mut dst = [0_u8; 4];
    convert(
        &src,
        layout(PixelFormat::Xrgb8888, 4),
        &mut dst,
        layout(PixelFormat::Abgr8888, 4),
        1,
        1,
    );
    // Abgr8888 is red, green, blue, alpha; a source without alpha is opaque.
    assert_eq!(dst, [30, 20, 10, 255]);
}

#[test]
fn alpha_is_kept_between_layouts_that_have_it_and_dropped_for_those_that_do_not() {
    let src = [1, 2, 3, 77];
    let mut with_alpha = [0_u8; 4];
    convert(
        &src,
        layout(PixelFormat::Argb8888, 4),
        &mut with_alpha,
        layout(PixelFormat::Abgr8888, 4),
        1,
        1,
    );
    assert_eq!(with_alpha, [3, 2, 1, 77]);
    let mut without = [9_u8; 4];
    convert(
        &src,
        layout(PixelFormat::Argb8888, 4),
        &mut without,
        layout(PixelFormat::Xbgr8888, 4),
        1,
        1,
    );
    assert_eq!(&without[..3], &[3, 2, 1]);
}

#[test]
fn a_buffer_that_is_too_small_is_filled_as_far_as_it_goes() {
    let src = [0_u8; 16];
    let mut dst = [0_u8; 8];
    let rows = convert(
        &src,
        layout(PixelFormat::Xrgb8888, 8),
        &mut dst,
        layout(PixelFormat::Xrgb8888, 8),
        2,
        2,
    );
    assert_eq!(rows, 1);
}
