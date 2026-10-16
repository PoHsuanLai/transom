//! Bytes from the operating system's random source.

use std::io::Read;

/// `N` random bytes, or `None` if the source cannot be read.
pub(crate) fn bytes<const N: usize>() -> Option<[u8; N]> {
    let mut bytes = [0_u8; N];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .ok()?;
    Some(bytes)
}

/// `N` random bytes as lower-case hex (`2 * N` characters), or `None` if the source cannot be read.
pub(crate) fn hex<const N: usize>() -> Option<String> {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let raw = bytes::<N>()?;
    Some(
        raw.iter()
            .flat_map(|b| [DIGITS[usize::from(b >> 4)], DIGITS[usize::from(b & 15)]])
            .map(char::from)
            .collect(),
    )
}
