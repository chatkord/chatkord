//! A camera's formats as PipeWire's pods: each it offers read into the modes it captures and how, and the one asked
//! offered back with the buffers it takes. Pure, so the shape is tested without a camera.

use std::io::Cursor;

use libspa as spa;
use spa::param::ParamType;
use spa::param::format::{FormatProperties, MediaSubtype, MediaType};
use spa::param::video::VideoFormat;
use spa::pod::serialize::PodSerializer;
use spa::pod::{
    CanonicalFixedSizedPod, ChoiceValue, Object, Property, PropertyFlags, Value, deserialize::PodDeserializer,
};
use spa::utils::{Choice, ChoiceEnum, ChoiceFlags, Fraction, Id, Rectangle, SpaTypes};

use crate::camera::{CameraError, Mode, Pixel};

/// The largest side a camera's picture may have; a mode past it is not offered.
const LARGEST_SIDE: u32 = 4096;

/// A mode a camera captures at, how it hands it over, and its rate exactly as it says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Offered {
    pub(super) pixel: Pixel,
    pub(super) mode: Mode,
    pub(super) rate: Fraction,
}

/// What was settled on: how pictures come, and their size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Settled {
    pub(super) pixel: Pixel,
    pub(super) width: u32,
    pub(super) height: u32,
}

/// Every value a property allows: itself, or each of a choice's (a range's default alone).
fn each<T: Copy>(value: &Value, bare: fn(&Value) -> Option<T>, chosen: fn(&ChoiceValue) -> Option<Vec<T>>) -> Vec<T> {
    match value {
        Value::Choice(choice) => chosen(choice).unwrap_or_default(),
        other => bare(other).into_iter().collect(),
    }
}

fn of_choice<T: CanonicalFixedSizedPod + Copy>(choice: &ChoiceEnum<T>) -> Vec<T> {
    match choice {
        ChoiceEnum::None(value) => vec![*value],
        ChoiceEnum::Enum { default, alternatives } => {
            std::iter::once(*default).chain(alternatives.iter().copied()).collect()
        }
        ChoiceEnum::Range { default, .. } | ChoiceEnum::Step { default, .. } => vec![*default],
        ChoiceEnum::Flags { default, .. } => vec![*default],
    }
}

fn ids(value: &Value) -> Vec<u32> {
    each(
        value,
        |bare| match bare {
            Value::Id(Id(raw)) => Some(*raw),
            _ => None,
        },
        |choice| match choice {
            ChoiceValue::Id(Choice(_, choice)) => Some(of_choice(choice).into_iter().map(|Id(raw)| raw).collect()),
            _ => None,
        },
    )
}

fn rectangles(value: &Value) -> Vec<Rectangle> {
    each(
        value,
        |bare| match bare {
            Value::Rectangle(rectangle) => Some(*rectangle),
            _ => None,
        },
        |choice| match choice {
            ChoiceValue::Rectangle(Choice(_, choice)) => Some(of_choice(choice)),
            _ => None,
        },
    )
}

fn fractions(value: &Value) -> Vec<Fraction> {
    each(
        value,
        |bare| match bare {
            Value::Fraction(fraction) => Some(*fraction),
            _ => None,
        },
        |choice| match choice {
            ChoiceValue::Fraction(Choice(_, choice)) => Some(of_choice(choice)),
            _ => None,
        },
    )
}

fn property(object: &Object, key: FormatProperties) -> Option<&Value> {
    object.properties.iter().find(|each| each.key == key.as_raw()).map(|each| &each.value)
}

/// The pixel a raw format's id names, of those taken here.
fn raw_pixel(id: u32) -> Option<Pixel> {
    if id == VideoFormat::NV12.as_raw() {
        Some(Pixel::Nv12)
    } else if id == VideoFormat::YUY2.as_raw() {
        Some(Pixel::Yuy2)
    } else {
        None
    }
}

/// The pixels a format object hands pictures over as: its raw formats taken here, or JPEG.
fn pixels(object: &Object) -> Vec<Pixel> {
    let subtype = property(object, FormatProperties::MediaSubtype).map(ids).unwrap_or_default();
    if subtype.contains(&MediaSubtype::Mjpg.as_raw()) {
        return vec![Pixel::Mjpeg];
    }
    if !subtype.contains(&MediaSubtype::Raw.as_raw()) {
        return Vec::new();
    }
    let formats = property(object, FormatProperties::VideoFormat).map(ids).unwrap_or_default();
    formats.into_iter().filter_map(raw_pixel).collect()
}

/// Whole pictures a second, from a rate as PipeWire says it; none for a still or a broken one.
fn per_second(rate: Fraction) -> Option<u32> {
    let whole = rate.num.checked_add(rate.denom / 2)?.checked_div(rate.denom)?;
    (whole > 0).then_some(whole)
}

/// The modes one `EnumFormat` pod offers, and how: none when it is not a video format taken here.
pub(super) fn offered(format: &[u8]) -> Vec<Offered> {
    let Ok((_, Value::Object(object))) = PodDeserializer::deserialize_any_from(format) else { return Vec::new() };
    let video = property(&object, FormatProperties::MediaType).map(ids).unwrap_or_default();
    if !video.contains(&MediaType::Video.as_raw()) {
        return Vec::new();
    }
    let sizes = property(&object, FormatProperties::VideoSize).map(rectangles).unwrap_or_default();
    let rates = property(&object, FormatProperties::VideoFramerate).map(fractions).unwrap_or_default();
    let mut out = Vec::new();
    for pixel in pixels(&object) {
        for size in sizes.iter().filter(|size| size.width <= LARGEST_SIDE && size.height <= LARGEST_SIDE) {
            for rate in &rates {
                let Some(frames_per_second) = per_second(*rate) else { continue };
                let mode = Mode { width: size.width, height: size.height, frames_per_second };
                out.push(Offered { pixel, mode, rate: *rate });
            }
        }
    }
    out
}

fn bytes(value: &Value) -> Result<Vec<u8>, CameraError> {
    PodSerializer::serialize(Cursor::new(Vec::new()), value)
        .map(|(out, _)| out.into_inner())
        .map_err(|_| CameraError::Format)
}

fn prop(key: impl Into<u32>, value: Value) -> Property {
    Property { key: key.into(), flags: PropertyFlags::empty(), value }
}

/// The format `offered` asked back, fixed: one pod, which the camera takes or refuses whole.
pub(super) fn offer(offered: &Offered) -> Result<Vec<u8>, CameraError> {
    let subtype = match offered.pixel {
        Pixel::Mjpeg => MediaSubtype::Mjpg,
        Pixel::Nv12 | Pixel::Yuy2 => MediaSubtype::Raw,
    };
    let mut properties = vec![
        prop(FormatProperties::MediaType.as_raw(), Value::Id(Id(MediaType::Video.as_raw()))),
        prop(FormatProperties::MediaSubtype.as_raw(), Value::Id(Id(subtype.as_raw()))),
    ];
    let raw = match offered.pixel {
        Pixel::Nv12 => Some(VideoFormat::NV12),
        Pixel::Yuy2 => Some(VideoFormat::YUY2),
        Pixel::Mjpeg => None,
    };
    if let Some(format) = raw {
        properties.push(prop(FormatProperties::VideoFormat.as_raw(), Value::Id(Id(format.as_raw()))));
    }
    let Mode { width, height, .. } = offered.mode;
    properties.push(prop(FormatProperties::VideoSize.as_raw(), Value::Rectangle(Rectangle { width, height })));
    properties.push(prop(FormatProperties::VideoFramerate.as_raw(), Value::Fraction(offered.rate)));
    bytes(&Value::Object(Object {
        type_: SpaTypes::ObjectParamFormat.as_raw(),
        id: ParamType::EnumFormat.as_raw(),
        properties,
    }))
}

/// A value the camera fixed: sent bare, or as a choice of exactly one.
fn single<T: Copy>(values: &[T]) -> Option<T> {
    match *values {
        [one] => Some(one),
        _ => None,
    }
}

/// The format the camera settled on, read from the `Format` param it sent; none for one not taken here.
pub(super) fn settled(param: &[u8]) -> Option<Settled> {
    let Ok((_, Value::Object(object))) = PodDeserializer::deserialize_any_from(param) else { return None };
    let pixel = single(&pixels(&object))?;
    let size = single(&property(&object, FormatProperties::VideoSize).map(rectangles)?)?;
    let fits = |side: u32| side > 0 && side <= LARGEST_SIDE && side.is_multiple_of(2);
    (fits(size.width) && fits(size.height)).then_some(Settled { pixel, width: size.width, height: size.height })
}

/// The pictures held here at once: the one being made NV12 and the newest, waiting.
const HELD: i32 = 2;

/// The buffers taken (memory the daemon maps, never a DMA-BUF: a camera's picture is read by the processor), and the
/// header every picture carries, which says when the camera marked one corrupt.
pub(super) fn buffers() -> Result<[Vec<u8>; 2], CameraError> {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        reason = "a C struct's size, far under i32::MAX"
    )]
    const HEADER: i32 = size_of::<spa::sys::spa_meta_header>() as i32;
    let memory = (1i32 << spa::sys::SPA_DATA_MemPtr) | (1i32 << spa::sys::SPA_DATA_MemFd);
    let buffers = bytes(&Value::Object(Object {
        type_: SpaTypes::ObjectParamBuffers.as_raw(),
        id: ParamType::Buffers.as_raw(),
        properties: vec![
            prop(
                spa::sys::SPA_PARAM_BUFFERS_dataType,
                Value::Choice(ChoiceValue::Int(Choice(
                    ChoiceFlags::empty(),
                    ChoiceEnum::Flags { default: memory, flags: vec![memory] },
                ))),
            ),
            prop(
                spa::sys::SPA_PARAM_BUFFERS_buffers,
                Value::Choice(ChoiceValue::Int(Choice(
                    ChoiceFlags::empty(),
                    ChoiceEnum::Range { default: HELD + 2, min: HELD + 1, max: 16 },
                ))),
            ),
        ],
    }))?;
    let header = bytes(&Value::Object(Object {
        type_: SpaTypes::ObjectParamMeta.as_raw(),
        id: ParamType::Meta.as_raw(),
        properties: vec![
            prop(spa::sys::SPA_PARAM_META_type, Value::Id(Id(spa::sys::SPA_META_Header))),
            prop(spa::sys::SPA_PARAM_META_size, Value::Int(HEADER)),
        ],
    }))?;
    Ok([buffers, header])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format(subtype: MediaSubtype, raw: Option<Value>, size: Value, rate: Value) -> Vec<u8> {
        let mut properties = vec![
            prop(FormatProperties::MediaType.as_raw(), Value::Id(Id(MediaType::Video.as_raw()))),
            prop(FormatProperties::MediaSubtype.as_raw(), Value::Id(Id(subtype.as_raw()))),
        ];
        if let Some(raw) = raw {
            properties.push(prop(FormatProperties::VideoFormat.as_raw(), raw));
        }
        properties.push(prop(FormatProperties::VideoSize.as_raw(), size));
        properties.push(prop(FormatProperties::VideoFramerate.as_raw(), rate));
        bytes(&Value::Object(Object {
            type_: SpaTypes::ObjectParamFormat.as_raw(),
            id: ParamType::EnumFormat.as_raw(),
            properties,
        }))
        .unwrap()
    }

    const fn mode(width: u32, height: u32, frames_per_second: u32) -> Mode {
        Mode { width, height, frames_per_second }
    }

    #[test]
    fn a_v4l2_camera_s_jpeg_and_yuy2_formats_are_its_modes_each_rate_counted() {
        // As Machine B's integrated camera says them (`pw-dump`): a size each, a choice of rates.
        let rates = Value::Choice(ChoiceValue::Fraction(Choice(
            ChoiceFlags::empty(),
            ChoiceEnum::Enum {
                default: Fraction { num: 30, denom: 1 },
                alternatives: vec![Fraction { num: 30, denom: 1 }, Fraction { num: 15, denom: 1 }],
            },
        )));
        let jpeg = format(MediaSubtype::Mjpg, None, Value::Rectangle(Rectangle { width: 1280, height: 720 }), rates);
        let modes: Vec<_> = offered(&jpeg).into_iter().map(|each| (each.pixel, each.mode)).collect();
        assert_eq!(
            modes,
            [
                (Pixel::Mjpeg, mode(1280, 720, 30)),
                (Pixel::Mjpeg, mode(1280, 720, 30)),
                (Pixel::Mjpeg, mode(1280, 720, 15))
            ]
        );
        let yuy2 = format(
            MediaSubtype::Raw,
            Some(Value::Id(Id(VideoFormat::YUY2.as_raw()))),
            Value::Rectangle(Rectangle { width: 1280, height: 720 }),
            Value::Fraction(Fraction { num: 10, denom: 1 }),
        );
        assert_eq!(
            offered(&yuy2).first().map(|each| (each.pixel, each.mode)),
            Some((Pixel::Yuy2, mode(1280, 720, 10)))
        );
    }

    #[test]
    fn a_rate_is_whole_pictures_a_second_and_a_format_not_taken_offers_nothing() {
        let ntsc = format(
            MediaSubtype::Raw,
            Some(Value::Id(Id(VideoFormat::NV12.as_raw()))),
            Value::Rectangle(Rectangle { width: 640, height: 480 }),
            Value::Fraction(Fraction { num: 30_000, denom: 1001 }),
        );
        let [one] = offered(&ntsc)[..] else { panic!("one mode") };
        assert_eq!(
            (one.pixel, one.mode, one.rate),
            (Pixel::Nv12, mode(640, 480, 30), Fraction { num: 30_000, denom: 1001 })
        );
        let rgb = format(
            MediaSubtype::Raw,
            Some(Value::Id(Id(VideoFormat::RGB.as_raw()))),
            Value::Rectangle(Rectangle { width: 640, height: 480 }),
            Value::Fraction(Fraction { num: 30, denom: 1 }),
        );
        assert!(offered(&rgb).is_empty(), "RGB is not taken");
        let still = format(
            MediaSubtype::Mjpg,
            None,
            Value::Rectangle(Rectangle { width: 640, height: 480 }),
            Value::Fraction(Fraction { num: 0, denom: 1 }),
        );
        assert!(offered(&still).is_empty(), "a rate of none is no mode");
        let huge = format(
            MediaSubtype::Mjpg,
            None,
            Value::Rectangle(Rectangle { width: 8192, height: 4320 }),
            Value::Fraction(Fraction { num: 30, denom: 1 }),
        );
        assert!(offered(&huge).is_empty(), "past the largest side");
        assert!(offered(&[1, 2, 3]).is_empty(), "not a pod");
    }

    #[test]
    fn what_is_offered_back_is_read_back_as_settled() {
        for pixel in [Pixel::Nv12, Pixel::Yuy2, Pixel::Mjpeg] {
            let asked = Offered { pixel, mode: mode(1280, 720, 30), rate: Fraction { num: 30, denom: 1 } };
            let pod = offer(&asked).unwrap();
            assert_eq!(offered(&pod), [asked], "the offer is one fixed mode");
            assert_eq!(settled(&pod), Some(Settled { pixel, width: 1280, height: 720 }));
        }
        let odd =
            offer(&Offered { pixel: Pixel::Nv12, mode: mode(641, 480, 30), rate: Fraction { num: 30, denom: 1 } });
        assert_eq!(settled(&odd.unwrap()), None, "an odd side is refused");
    }
}
