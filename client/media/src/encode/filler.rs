//! The filler units a constant rate pads a picture with, taken out by FFmpeg's `filter_units`: they carry no
//! picture, and a WebRTC receiver keeps no buffer model that needs them.

use super::EncodeError;
use crate::av::{AvError, CodecContext, Packet, UnitFilter};
use crate::probe::Codec;
use std::ffi::CStr;

/// The filter of one encoder's stream; made again with each encoder a session opens.
pub(crate) struct Filler(UnitFilter);

/// The unit type that is filler in each codec's stream: H.264's NAL 12, AV1's padding OBU.
const fn filler_type(codec: Codec) -> &'static CStr {
    match codec {
        Codec::H264 => c"12",
        Codec::Av1 => c"15",
    }
}

impl Filler {
    /// The filter for the stream of `codec` that the open encoder `context` writes.
    pub(crate) fn new(codec: Codec, context: &CodecContext) -> Result<Filler, EncodeError> {
        UnitFilter::new(context, filler_type(codec)).map(Filler).map_err(|error| match error {
            AvError::Unsupported => EncodeError::Open,
            other => EncodeError::Encoder(other),
        })
    }

    /// `packet` without its filler, in place; false when nothing was left of it.
    pub(crate) fn take(&mut self, packet: &mut Packet) -> Result<bool, EncodeError> {
        self.0.send(packet).map_err(EncodeError::Encoder)?;
        // `filter_units` gives at most one packet for each it is sent, so one receive empties it.
        match self.0.receive(packet) {
            Ok(()) => Ok(true),
            Err(AvError::Again) => Ok(false),
            Err(error) => Err(EncodeError::Encoder(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::av::StreamCodec;

    /// The NAL units of an Annex B stream, each with its header and without its start code.
    fn units(stream: &[u8]) -> Vec<&[u8]> {
        let starts: Vec<usize> =
            stream.windows(3).enumerate().filter(|(_, w)| *w == [0, 0, 1]).map(|(at, _)| at + 3).collect();
        starts
            .iter()
            .enumerate()
            .map(|(n, start)| {
                let end = starts.get(n + 1).map_or(stream.len(), |next| next - 3);
                let unit = &stream[*start..end];
                // A four-byte start code leaves a zero at the end of the unit before it.
                unit.strip_suffix(&[0]).unwrap_or(unit)
            })
            .collect()
    }

    /// `stream` through the filler filter of `codec`.
    fn filtered(codec: Codec, coded: StreamCodec, stream: &[u8]) -> Vec<u8> {
        let mut context = CodecContext::new(None).expect("a codec context");
        context.describe(coded);
        let mut filler = Filler::new(codec, &context).expect("the filter");
        drop(context);
        let mut packet = Packet::copied(stream).expect("a packet");
        assert!(filler.take(&mut packet).expect("filtered"), "a picture is left");
        packet.bytes().to_vec()
    }

    #[test]
    fn the_filler_between_two_slices_goes_and_the_slices_stay_as_they_were() {
        let (first, filler, second) = ([0x21, 0x9A, 0x40, 0x11], [0x0C, 0xFF, 0xFF, 0xFF, 0x80], [0x01, 0x9B, 0x22]);
        let stream = [&[0, 0, 0, 1][..], &first, &[0, 0, 1], &filler, &[0, 0, 1], &second].concat();
        let out = filtered(Codec::H264, StreamCodec::H264, &stream);
        assert_eq!(units(&out), [&first[..], &second[..]]);
    }

    mod lab {
        use super::*;
        use crate::encode::{Nv12, Session, Settings};
        use std::num::NonZeroU32;

        /// A flat picture at a rate far above what it needs, so a constant rate has bits to pad.
        const SETTINGS: Settings = Settings {
            width: 640,
            height: 360,
            frames_per_second: 30,
            bits_per_second: 8_000_000,
            key_frame_every: NonZeroU32::new(30),
        };

        /// The bytes of `unit`'s filler units, read from the stream itself: the filter writes the units it keeps
        /// again, and may shorten a start code, so its output's length is not the filler's.
        fn filler_in(codec: Codec, unit: &[u8]) -> usize {
            match codec {
                Codec::H264 => units(unit)
                    .iter()
                    .filter(|nal| nal.first().is_some_and(|h| h & 0x1F == 12))
                    .map(|nal| nal.len())
                    .sum(),
                Codec::Av1 => obus(unit).iter().filter(|(kind, _)| *kind == 15).map(|(_, obu)| obu.len()).sum(),
            }
        }

        /// The OBUs of an AV1 temporal unit in the low-overhead format, each with its type.
        fn obus(mut stream: &[u8]) -> Vec<(u8, &[u8])> {
            let mut found = Vec::new();
            while let Some(&header) = stream.first() {
                let mut at = 1 + usize::from(header & 0b100 != 0);
                let size = if header & 0b10 == 0 {
                    stream.len() - at
                } else {
                    let (mut size, mut shift) = (0usize, 0);
                    while let Some(&byte) = stream.get(at) {
                        size |= usize::from(byte & 0x7F) << shift;
                        (at, shift) = (at + 1, shift + 7);
                        if byte & 0x80 == 0 {
                            break;
                        }
                    }
                    size
                };
                let end = (at + size).min(stream.len());
                found.push(((header >> 3) & 0x0F, &stream[..end]));
                stream = &stream[end..];
            }
            found
        }

        /// An encoder whose output the filter is not run on writes no filler: its rate control is not a constant
        /// one that pads, which the filter is only for.
        #[test]
        fn no_encoder_s_units_carry_filler_whether_or_not_the_filter_runs_on_them() {
            tier::lab();
            let flat = Nv12::new(SETTINGS.width, SETTINGS.height, vec![128; SETTINGS.width * SETTINGS.height * 3 / 2])
                .unwrap();
            for encoder in crate::probe::survey().encoders {
                let mut session = Session::open(&encoder, SETTINGS).unwrap();
                let mut units = Vec::new();
                for _ in 0..60 {
                    units.extend(session.encode(&flat).unwrap());
                }
                units.extend(session.finish().unwrap());
                let filler: usize = units.iter().map(|unit| filler_in(encoder.codec(), &unit.data)).sum();
                let bytes: usize = units.iter().map(|unit| unit.data.len()).sum();
                eprintln!(
                    "{encoder}: {} units, {bytes} bytes, {filler} of filler left, filter run: {}",
                    units.len(),
                    encoder.name.family().pads()
                );
                assert_eq!(filler, 0, "{encoder} sent filler");
            }
        }
    }
}
