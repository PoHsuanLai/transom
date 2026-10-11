//! Pixel layout conversion between the capture's shared-memory formats.
//!
//! A consumer of the stream picks the layout it wants from the four 32-bit ones offered; the
//! compositor hands over whichever it likes. [`convert`] copies a frame into a destination
//! buffer in the layout the consumer picked.

use crate::capture::PixelFormat;

/// Where each channel sits among the four bytes of a pixel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Lanes {
    red: usize,
    green: usize,
    blue: usize,
    /// `None` when the fourth byte is padding.
    alpha: Option<usize>,
}

fn lanes(format: PixelFormat) -> Lanes {
    // The `wl_shm` names read the 32-bit word most significant byte first; the words are little
    // endian in memory, so `Argb8888` is the bytes blue, green, red, alpha.
    match format {
        PixelFormat::Argb8888 => Lanes {
            red: 2,
            green: 1,
            blue: 0,
            alpha: Some(3),
        },
        PixelFormat::Xrgb8888 => Lanes {
            red: 2,
            green: 1,
            blue: 0,
            alpha: None,
        },
        PixelFormat::Abgr8888 => Lanes {
            red: 0,
            green: 1,
            blue: 2,
            alpha: Some(3),
        },
        PixelFormat::Xbgr8888 => Lanes {
            red: 0,
            green: 1,
            blue: 2,
            alpha: None,
        },
    }
}

/// A pixel layout, rows and size: what one side of a copy looks like.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    /// The pixel layout.
    pub format: PixelFormat,
    /// Bytes per row.
    pub stride: usize,
}

/// Copy `width` x `height` pixels from `src` (laid out as `from`) into `dst` (laid out as `to`).
///
/// A destination alpha with no source alpha is opaque. Rows or pixels that do not fit in either
/// buffer are left out, so a buffer that is too small is filled as far as it goes and nothing
/// panics. Returns the number of rows written.
pub fn convert(
    src: &[u8],
    from: Layout,
    dst: &mut [u8],
    to: Layout,
    width: usize,
    height: usize,
) -> usize {
    let row_bytes = width * 4;
    let same = from.format == to.format;
    let (a, b) = (lanes(from.format), lanes(to.format));
    let mut written = 0;
    for row in 0..height {
        let (s, d) = (row * from.stride, row * to.stride);
        let (Some(s_row), Some(d_row)) = (src.get(s..s + row_bytes), dst.get_mut(d..d + row_bytes))
        else {
            break;
        };
        if same {
            d_row.copy_from_slice(s_row);
        } else {
            for (s_px, d_px) in s_row
                .as_chunks::<4>()
                .0
                .iter()
                .zip(d_row.as_chunks_mut::<4>().0)
            {
                d_px[b.red] = s_px[a.red];
                d_px[b.green] = s_px[a.green];
                d_px[b.blue] = s_px[a.blue];
                if let Some(at) = b.alpha {
                    d_px[at] = a.alpha.map_or(0xff, |from_at| s_px[from_at]);
                }
            }
        }
        written += 1;
    }
    written
}
