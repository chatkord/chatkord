//! The fuzz target's entry for the coded video a server relays to the client.

use crate::size::MAX_PIXELS;
use crate::{Codec, Coded, Decoder, Sources};

/// A relayed stream into the software decoder: the first byte picks H.264 or AV1, the rest is units, each behind a
/// two-byte length. Whatever the bytes, a unit is decoded or refused, and every picture that comes out is within the
/// largest a watcher takes.
pub fn decode_bitstream(data: &[u8]) {
    let Some((&picked, mut rest)) = data.split_first() else { return };
    let codec = if picked % 2 == 0 { Codec::H264 } else { Codec::Av1 };
    let Ok(mut decoder) = Decoder::open(codec, &Sources::default()) else { return };
    let mut pictures = Vec::new();
    let mut pts = 0;
    while let Some((length, after)) = rest.split_first_chunk::<2>() {
        let length = usize::from(u16::from_be_bytes(*length)).min(after.len());
        let (unit, next) = after.split_at(length);
        rest = next;
        if let Ok(coded) = Coded::copied(unit)
            && let Ok(decoded) = decoder.decode(&coded, pts)
        {
            pictures.extend(decoded);
        }
        pts += 1;
    }
    if let Ok(decoded) = decoder.finish() {
        pictures.extend(decoded);
    }
    for decoded in pictures {
        let pixels = i64::try_from(decoded.picture.width())
            .unwrap_or(i64::MAX)
            .saturating_mul(i64::try_from(decoded.picture.height()).unwrap_or(i64::MAX));
        assert!(pixels <= MAX_PIXELS, "a picture of {pixels} pixels came out of a decoder opened for {MAX_PIXELS}");
    }
}

#[cfg(test)]
mod tests {
    use super::decode_bitstream;
    use std::path::{Path, PathBuf};

    fn seeds(target: &str) -> std::io::Result<Vec<(PathBuf, Vec<u8>)>> {
        proto::fuzz::seeds(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/seeds"), target)
    }

    #[test]
    fn every_kept_stream_is_decoded_or_refused_within_the_largest_picture() {
        for (_, input) in seeds("decode_bitstream").expect("the seeds") {
            decode_bitstream(&input);
        }
    }
}
