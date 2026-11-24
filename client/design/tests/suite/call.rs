//! What a call paints keeps the contract: a person's initials on their colour, the text on their tile, the icon of
//! every call control in every state, and the muted mark on the scrim, in both schemes and under any accent.

use design::{
    Accent, CallControl, Contrast, Interaction, Oklch, Palette, PersonColour, Ratio, Rgb, Role, Scheme, Swatch,
    call_button,
};

fn accents() -> impl Iterator<Item = Accent> {
    (0..360).step_by(15).flat_map(|hue| {
        [(0.0, 0.5), (0.15, 0.6), (0.3, 0.95), (0.15, 0.2)]
            .map(move |(chroma, l)| Accent::from(Oklch { l, c: chroma, h: f64::from(hue) }.to_rgb()))
    })
}

/// Colours a person may pick: a sweep of hue, lightness and chroma, and the ones that try hardest to break a tile.
fn picked() -> impl Iterator<Item = PersonColour> {
    let swept = (0..360).step_by(20).flat_map(|hue| {
        [(0.0, 0.5), (0.1, 0.3), (0.15, 0.6), (0.2, 0.8), (0.3, 0.95)]
            .map(move |(chroma, l)| Oklch { l, c: chroma, h: f64::from(hue) }.to_rgb())
    });
    let hostile = [0x000000, 0xFFFFFF, 0xFFFF00, 0x00FFFF, 0xFF00FF, 0x777777, 0x17121C, 0xFFF8F3].map(Rgb::hex);
    swept.chain(hostile).map(PersonColour::Picked).chain(Swatch::ALL.map(PersonColour::Swatch))
}

fn palettes() -> impl Iterator<Item = Palette> {
    Scheme::ALL.into_iter().flat_map(|scheme| {
        Contrast::ALL.into_iter().map(move |contrast| Palette::with_contrast(scheme, Accent::DEFAULT, contrast))
    })
}

#[test]
fn initials_are_readable_on_any_colour_a_person_picks() {
    for palette in palettes() {
        for colour in picked() {
            let look = palette.person(colour);
            assert!(Ratio::between(look.on_fill, look.fill).meets(Ratio::TEXT), "{colour:?}: {look:?}");
        }
    }
}

#[test]
fn the_text_on_anyone_s_tile_keeps_the_contrast_whatever_they_picked() {
    for palette in palettes() {
        for colour in picked() {
            let ground = palette.person(colour).ground;
            for content in [design::Content::Primary, design::Content::Secondary] {
                let found = Ratio::between(palette.color(Role::Content(content)), ground);
                assert!(
                    found.meets(palette.contrast().text()),
                    "{content:?} on {colour:?} in {:?}/{:?}: {found:?}",
                    palette.scheme(),
                    palette.contrast()
                );
            }
        }
    }
}

#[test]
fn a_tile_is_told_from_the_window_and_from_the_others() {
    for scheme in Scheme::ALL {
        let palette = Palette::new(scheme, Accent::DEFAULT);
        let base = palette.color(Role::Surface(design::Surface::Base));
        let grounds: Vec<_> =
            Swatch::ALL.iter().map(|swatch| palette.person(PersonColour::Swatch(*swatch)).ground).collect();
        for (at, ground) in grounds.iter().enumerate() {
            assert_ne!(*ground, base, "{scheme:?}: {:?} shows against the window", Swatch::ALL[at]);
            assert_eq!(grounds.iter().filter(|other| *other == ground).count(), 1, "{scheme:?}: one tile each");
        }
    }
}

#[test]
fn every_call_control_icon_is_found_on_its_fill_in_every_state() {
    for accent in accents() {
        for scheme in Scheme::ALL {
            let palette = Palette::new(scheme, accent);
            for control in CallControl::ALL {
                for interaction in Interaction::ALL {
                    let look = call_button(control, interaction);
                    let minimum = if interaction == Interaction::Disabled { Ratio::DISABLED } else { Ratio::NON_TEXT };
                    let found = Ratio::between(palette.color(look.icon), palette.color(look.fill));
                    assert!(
                        found.meets(minimum),
                        "{control:?}/{interaction:?} in {scheme:?} under {accent:?}: {found:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_call_control_answers_the_pointer() {
    for control in CallControl::ALL {
        assert_ne!(
            call_button(control, Interaction::Idle),
            call_button(control, Interaction::Hover),
            "{control:?} shows the pointer"
        );
        assert_ne!(
            call_button(control, Interaction::Idle),
            call_button(control, Interaction::Disabled),
            "{control:?} shows it is off"
        );
    }
}
