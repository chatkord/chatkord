//! The PipeWire negotiation as bytes: what we offer and how a format the compositor agreed to is read back. Pure,
//! so the shape is tested without a compositor.

use std::io::Cursor;

use libspa as spa;
use spa::param::ParamType;
use spa::param::format::{FormatProperties, MediaSubtype, MediaType};
use spa::param::video::VideoFormat;
use spa::pod::serialize::PodSerializer;
use spa::pod::{ChoiceValue, Object, Property, PropertyFlags, Value, deserialize::PodDeserializer};
use spa::utils::{Choice, ChoiceEnum, ChoiceFlags, Fraction, Id, Rectangle, SpaTypes};

use super::Modifier;
use crate::{CaptureError, Pixels};

/// The largest side a captured picture may have; a compositor that says more is refused.
const LARGEST_SIDE: u32 = 16_384;

/// A format the compositor settled on, or is still choosing a modifier for.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Negotiated {
    pub(crate) pixels: Pixels,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) modifier: ModifierChoice,
}

/// The modifier of a negotiated format.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ModifierChoice {
    /// Still open: the compositor, which makes the buffers, fixes one itself and says so in a new format.
    Open,
    /// Settled.
    Fixed(Modifier),
    /// There is none: the buffers are not DMA-BUFs.
    Absent,
}

fn bytes(value: &Value) -> Result<Vec<u8>, CaptureError> {
    PodSerializer::serialize(Cursor::new(Vec::new()), value)
        .map(|(out, _)| out.into_inner())
        .map_err(|_| CaptureError::Stream)
}

fn prop(key: FormatProperties, value: Value) -> Property {
    Property { key: key.as_raw(), flags: PropertyFlags::empty(), value }
}

fn id(raw: u32) -> Value {
    Value::Id(Id(raw))
}

fn pixel_id(pixels: Pixels) -> u32 {
    match pixels {
        Pixels::Bgrx => VideoFormat::BGRx.as_raw(),
        Pixels::Bgra => VideoFormat::BGRA.as_raw(),
    }
}

/// The frame rates the compositor may send at: variable (it sends on damage) and never more than `max_fps`, the
/// share's own rate, so a screen faster than the share is not drawn into the cast for pictures the share drops.
fn rates(max_fps: u32) -> [Property; 2] {
    let range = |default: u32| {
        Value::Choice(ChoiceValue::Fraction(Choice(
            ChoiceFlags::empty(),
            ChoiceEnum::Range {
                default: Fraction { num: default, denom: 1 },
                min: Fraction { num: 0, denom: 1 },
                max: Fraction { num: max_fps, denom: 1 },
            },
        )))
    };
    [prop(FormatProperties::VideoFramerate, range(0)), prop(FormatProperties::VideoMaxFramerate, range(max_fps))]
}

fn head() -> [Property; 2] {
    [
        prop(FormatProperties::MediaType, id(MediaType::Video.as_raw())),
        prop(FormatProperties::MediaSubtype, id(MediaSubtype::Raw.as_raw())),
    ]
}

fn format_object(properties: Vec<Property>) -> Value {
    Value::Object(Object {
        type_: SpaTypes::ObjectParamFormat.as_raw(),
        id: ParamType::EnumFormat.as_raw(),
        properties,
    })
}

/// What we offer: BGRx or BGRA of any size, up to `max_fps`, in one of `modifiers` (`MANDATORY`), which the
/// compositor may narrow by sending its own list back (`DONT_FIXATE`). Never without a modifier: no shared memory.
pub(crate) fn offer(modifiers: &[Modifier], max_fps: u32) -> Result<Vec<u8>, CaptureError> {
    let first = modifiers.first().ok_or(CaptureError::NoDmaBuf)?;
    let mut properties = Vec::from(head());
    properties.push(prop(
        FormatProperties::VideoFormat,
        Value::Choice(ChoiceValue::Id(Choice(
            ChoiceFlags::empty(),
            ChoiceEnum::Enum {
                default: Id(pixel_id(Pixels::Bgrx)),
                alternatives: vec![Id(pixel_id(Pixels::Bgrx)), Id(pixel_id(Pixels::Bgra))],
            },
        ))),
    ));
    properties.push(prop(
        FormatProperties::VideoSize,
        Value::Choice(ChoiceValue::Rectangle(Choice(
            ChoiceFlags::empty(),
            ChoiceEnum::Range {
                default: Rectangle { width: 1920, height: 1080 },
                min: Rectangle { width: 1, height: 1 },
                max: Rectangle { width: LARGEST_SIDE, height: LARGEST_SIDE },
            },
        ))),
    ));
    properties.extend(rates(max_fps));
    properties.push(Property {
        key: FormatProperties::VideoModifier.as_raw(),
        flags: PropertyFlags::MANDATORY | PropertyFlags::DONT_FIXATE,
        value: Value::Choice(ChoiceValue::Long(Choice(
            ChoiceFlags::empty(),
            ChoiceEnum::Enum {
                default: first.raw().cast_signed(),
                alternatives: modifiers.iter().map(|m| m.raw().cast_signed()).collect(),
            },
        ))),
    });
    bytes(&format_object(properties))
}

/// The compositor's buffers held here at once: the one the encoder reads and the one waiting for one's own card.
const HELD: i32 = 2;

/// The buffers we accept (DMA-BUFs only, as many as [`HELD`] needs) and the header every frame carries, which says
/// when the compositor marked one corrupt.
pub(crate) fn buffers() -> Result<[Vec<u8>; 2], CaptureError> {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        reason = "a C struct's size, far under i32::MAX"
    )]
    const HEADER: i32 = std::mem::size_of::<spa::sys::spa_meta_header>() as i32;
    #[expect(clippy::cast_possible_truncation, clippy::cast_possible_wrap, reason = "a count far under i32::MAX")]
    const MOST: i32 = crate::LENT_AT_MOST as i32;
    let dma_buf = 1i32 << spa::sys::SPA_DATA_DmaBuf;
    let buffers = bytes(&Value::Object(Object {
        type_: SpaTypes::ObjectParamBuffers.as_raw(),
        id: ParamType::Buffers.as_raw(),
        properties: vec![
            Property {
                key: spa::sys::SPA_PARAM_BUFFERS_dataType,
                flags: PropertyFlags::empty(),
                value: Value::Choice(ChoiceValue::Int(Choice(
                    ChoiceFlags::empty(),
                    ChoiceEnum::Flags { default: dma_buf, flags: vec![dma_buf] },
                ))),
            },
            // Enough that the compositor always has one to draw into besides those held here, and no more than the
            // encoder keeps imported, so no buffer is imported again for being one too many.
            Property {
                key: spa::sys::SPA_PARAM_BUFFERS_buffers,
                flags: PropertyFlags::empty(),
                value: Value::Choice(ChoiceValue::Int(Choice(
                    ChoiceFlags::empty(),
                    ChoiceEnum::Range { default: HELD + 2, min: HELD + 1, max: MOST },
                ))),
            },
        ],
    }))?;
    let header = bytes(&Value::Object(Object {
        type_: SpaTypes::ObjectParamMeta.as_raw(),
        id: ParamType::Meta.as_raw(),
        properties: vec![
            Property {
                key: spa::sys::SPA_PARAM_META_type,
                flags: PropertyFlags::empty(),
                value: id(spa::sys::SPA_META_Header),
            },
            Property { key: spa::sys::SPA_PARAM_META_size, flags: PropertyFlags::empty(), value: Value::Int(HEADER) },
        ],
    }))?;
    Ok([buffers, header])
}

/// A value the compositor fixed: sent bare, or as a choice of exactly one, which is how it says it.
fn single_id(value: &Value) -> Option<u32> {
    match value {
        Value::Id(Id(raw)) | Value::Choice(ChoiceValue::Id(Choice(_, ChoiceEnum::None(Id(raw))))) => Some(*raw),
        _ => None,
    }
}

fn single_rectangle(value: &Value) -> Option<Rectangle> {
    match value {
        Value::Rectangle(rectangle) | Value::Choice(ChoiceValue::Rectangle(Choice(_, ChoiceEnum::None(rectangle)))) => {
            Some(*rectangle)
        }
        _ => None,
    }
}

/// Reads the format the compositor sent: refused when it is not a picture we can encode.
pub(crate) fn parse(param: &[u8]) -> Result<Negotiated, CaptureError> {
    let Ok((_, Value::Object(object))) = PodDeserializer::deserialize_any_from(param) else {
        return Err(CaptureError::Format);
    };
    let find = |key: FormatProperties| object.properties.iter().find(|p| p.key == key.as_raw());

    let pixels = match find(FormatProperties::VideoFormat).and_then(|p| single_id(&p.value)) {
        Some(raw) if raw == pixel_id(Pixels::Bgrx) => Pixels::Bgrx,
        Some(raw) if raw == pixel_id(Pixels::Bgra) => Pixels::Bgra,
        _ => return Err(CaptureError::Format),
    };
    let (width, height) = match find(FormatProperties::VideoSize).and_then(|p| single_rectangle(&p.value)) {
        Some(Rectangle { width, height })
            if (1..=LARGEST_SIDE).contains(&width) && (1..=LARGEST_SIDE).contains(&height) =>
        {
            (width, height)
        }
        _ => return Err(CaptureError::Format),
    };
    let modifier = match find(FormatProperties::VideoModifier) {
        None => ModifierChoice::Absent,
        Some(p) => match &p.value {
            Value::Long(raw) => ModifierChoice::Fixed(Modifier::from_raw(raw.cast_unsigned())),
            Value::Choice(ChoiceValue::Long(Choice(_, ChoiceEnum::Enum { .. })))
                if p.flags.contains(PropertyFlags::DONT_FIXATE) =>
            {
                ModifierChoice::Open
            }
            Value::Choice(ChoiceValue::Long(Choice(
                _,
                ChoiceEnum::Enum { default, .. } | ChoiceEnum::None(default),
            ))) => ModifierChoice::Fixed(Modifier::from_raw(default.cast_unsigned())),
            _ => return Err(CaptureError::Format),
        },
    };
    Ok(Negotiated { pixels, width, height, modifier })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TILED: Modifier = Modifier::from_raw(0x0200_0000_0000_0001);

    fn read(raw: &[u8]) -> Result<Negotiated, CaptureError> {
        parse(raw)
    }

    #[test]
    fn an_offer_without_a_modifier_is_refused() {
        assert_eq!(offer(&[], 60), Err(CaptureError::NoDmaBuf));
    }

    #[test]
    fn the_compositor_still_choosing_is_an_open_list() {
        // A compositor that narrowed the pick to its own list sends a fixed size with the modifier left open; it fixes
        // one itself next.
        let mut properties = Vec::from(head());
        properties.push(prop(FormatProperties::VideoFormat, id(pixel_id(Pixels::Bgrx))));
        properties.push(prop(FormatProperties::VideoSize, Value::Rectangle(Rectangle { width: 2560, height: 1440 })));
        properties.push(Property {
            key: FormatProperties::VideoModifier.as_raw(),
            flags: PropertyFlags::MANDATORY | PropertyFlags::DONT_FIXATE,
            value: Value::Choice(ChoiceValue::Long(Choice(
                ChoiceFlags::empty(),
                ChoiceEnum::Enum {
                    default: TILED.raw().cast_signed(),
                    alternatives: vec![TILED.raw().cast_signed(), Modifier::LINEAR.raw().cast_signed()],
                },
            ))),
        });
        let open = read(&bytes(&format_object(properties)).unwrap()).unwrap();
        assert_eq!(open.modifier, ModifierChoice::Open);
        assert_eq!((open.width, open.height), (2560, 1440));
    }

    #[test]
    fn a_value_the_compositor_fixed_may_come_as_a_choice_of_one() {
        // What KWin sends: the format and the size each as `Choice::None(value)`.
        let mut properties = Vec::from(head());
        properties.push(prop(
            FormatProperties::VideoFormat,
            Value::Choice(ChoiceValue::Id(Choice(ChoiceFlags::empty(), ChoiceEnum::None(Id(pixel_id(Pixels::Bgrx)))))),
        ));
        properties.push(prop(
            FormatProperties::VideoSize,
            Value::Choice(ChoiceValue::Rectangle(Choice(
                ChoiceFlags::empty(),
                ChoiceEnum::None(Rectangle { width: 3840, height: 2160 }),
            ))),
        ));
        let negotiated = read(&bytes(&format_object(properties)).unwrap()).unwrap();
        assert_eq!((negotiated.pixels, negotiated.width, negotiated.height), (Pixels::Bgrx, 3840, 2160));
    }

    #[test]
    fn a_format_with_no_modifier_is_absent() {
        let mut properties = Vec::from(head());
        properties.push(prop(FormatProperties::VideoFormat, id(pixel_id(Pixels::Bgra))));
        properties.push(prop(FormatProperties::VideoSize, Value::Rectangle(Rectangle { width: 64, height: 64 })));
        let shared_memory = read(&bytes(&format_object(properties)).unwrap()).unwrap();
        assert_eq!(shared_memory.modifier, ModifierChoice::Absent);
    }

    #[test]
    fn a_size_that_cannot_be_encoded_is_refused() {
        for (width, height) in [(0, 1080), (1920, 0), (LARGEST_SIDE + 1, 1080)] {
            let mut properties = Vec::from(head());
            properties.push(prop(FormatProperties::VideoFormat, id(pixel_id(Pixels::Bgrx))));
            properties.push(prop(FormatProperties::VideoSize, Value::Rectangle(Rectangle { width, height })));
            assert_eq!(
                read(&bytes(&format_object(properties)).unwrap()),
                Err(CaptureError::Format),
                "{width}x{height}"
            );
        }
    }

    #[test]
    fn a_pixel_format_we_do_not_offer_is_refused() {
        let mut properties = Vec::from(head());
        properties.push(prop(FormatProperties::VideoFormat, id(VideoFormat::NV12.as_raw())));
        properties.push(prop(FormatProperties::VideoSize, Value::Rectangle(Rectangle { width: 64, height: 64 })));
        assert_eq!(read(&bytes(&format_object(properties)).unwrap()), Err(CaptureError::Format));
    }

    #[test]
    fn the_offer_asks_for_no_more_pictures_than_the_share_sends() {
        let offered = offer(&[TILED], 30).unwrap();
        let (_, Value::Object(object)) = PodDeserializer::deserialize_any_from(&offered).unwrap() else {
            panic!("an offer is an object");
        };
        for key in [FormatProperties::VideoFramerate, FormatProperties::VideoMaxFramerate] {
            let rate = object.properties.iter().find(|p| p.key == key.as_raw()).expect("the rate is offered");
            let Value::Choice(ChoiceValue::Fraction(Choice(_, ChoiceEnum::Range { max, .. }))) = &rate.value else {
                panic!("a rate is a range");
            };
            assert_eq!((max.num, max.denom), (30, 1));
        }
    }

    #[test]
    fn the_cast_is_asked_for_a_buffer_to_draw_into_beyond_those_held_and_no_more_than_are_kept() {
        let [asked, _] = buffers().unwrap();
        let (_, Value::Object(object)) = PodDeserializer::deserialize_any_from(&asked).unwrap() else {
            panic!("the buffers asked are an object");
        };
        let count = object.properties.iter().find(|p| p.key == spa::sys::SPA_PARAM_BUFFERS_buffers).expect("a count");
        let Value::Choice(ChoiceValue::Int(Choice(_, ChoiceEnum::Range { default, min, max }))) = count.value else {
            panic!("a count is a range");
        };
        assert!(min > HELD, "the compositor would be left no buffer to draw into");
        assert!(default > min, "one spare beyond the least, so a late return never stalls the cast");
        assert_eq!(usize::try_from(max).ok(), Some(crate::LENT_AT_MOST));
    }

    #[test]
    fn the_offer_and_the_buffers_serialise() {
        assert!(!offer(&[TILED, Modifier::LINEAR], 60).unwrap().is_empty());
        assert!(buffers().unwrap().iter().all(|b| !b.is_empty()));
    }
}
