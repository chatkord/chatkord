//! The contrast contract against every accent a server could name: every hue, from grey to as
//! vivid as the screen shows, from near black to near white, and the colours most likely to break
//! it. A palette is built for each, in both schemes, and must meet every pair.

use design::{
    Accent, AccentRole, Border, Content, Contrast, DestructiveRole, Ground, Oklch, Palette, Ratio, Rgb, Role, Scheme,
    StatusRole, Surface, contract, readable_on, violations,
};

fn assert_meets_contract(accent: Accent) {
    for scheme in Scheme::ALL {
        for contrast in Contrast::ALL {
            let palette = Palette::with_contrast(scheme, accent, contrast);
            let broken = violations(&palette);
            assert!(broken.is_empty(), "{accent:?} in {scheme:?} at {contrast:?}: {broken:#?}");
        }
    }
}

/// Seeds across the hue circle at several chromas and lightnesses, each as a screen shows it.
fn sweep() -> impl Iterator<Item = Accent> {
    (0..360).flat_map(|hue| {
        [0.0, 0.03, 0.08, 0.15, 0.30].into_iter().flat_map(move |chroma| {
            [0.25, 0.6, 0.95].map(move |l| Accent::from(Oklch { l, c: chroma, h: f64::from(hue) }.to_rgb()))
        })
    })
}

#[test]
fn every_accent_on_the_hue_circle_meets_the_contract() {
    for accent in sweep() {
        assert_meets_contract(accent);
    }
}

#[test]
fn the_accents_most_likely_to_break_it_meet_the_contract() {
    let surfaces = Scheme::ALL.into_iter().flat_map(|scheme| {
        Surface::ALL.map(|surface| Palette::new(scheme, Accent::DEFAULT).color(Role::Surface(surface)))
    });
    let hostile = [0xFFFF00, 0xFF0000, 0x00FF00, 0x0000FF, 0x808080, 0x000000, 0x010101, 0xFFFFFF, 0xFEFEFE, 0x7F7F00]
        .map(Rgb::hex)
        .into_iter()
        .chain(surfaces);
    for seed in hostile {
        assert_meets_contract(Accent::from(seed));
    }
    assert_meets_contract(Accent::DEFAULT);
}

/// The derivation is what makes an accent safe: yellow as chosen is no fill on a light window,
/// so a derivation that handed the seed through unchanged would fail the sweep above.
#[test]
fn an_accent_as_chosen_can_break_the_contract_and_the_derived_one_does_not() {
    let yellow = Rgb::hex(0xFFFF00);
    let light = Palette::new(Scheme::Light, Accent::from(yellow));
    let window = light.color(Role::Surface(Surface::Base));
    assert!(!Ratio::between(yellow, window).meets(Ratio::NON_TEXT));
    let fill = light.color(Role::Accent(AccentRole::Fill));
    assert_ne!(fill, yellow);
    assert!(Ratio::between(fill, window).meets(Ratio::NON_TEXT));
}

/// A server's colour stays recognisable: where it has a hue worth keeping, the fill keeps it.
#[test]
fn a_vivid_accent_keeps_its_hue() {
    for hue in (0..360).step_by(5) {
        let seed = Oklch { l: 0.6, c: 0.15, h: f64::from(hue) }.to_rgb();
        let wanted = seed.to_oklch().h;
        for scheme in Scheme::ALL {
            let fill = Palette::new(scheme, Accent::from(seed)).color(Role::Accent(AccentRole::Fill)).to_oklch();
            if fill.c >= 0.05 {
                let off = (fill.h - wanted + 540.0).rem_euclid(360.0) - 180.0;
                assert!(off.abs() <= 4.0, "hue {hue} in {scheme:?}: {fill:?}");
            }
        }
    }
}

#[test]
fn a_fill_answers_hover_and_press_with_a_visible_change() {
    for accent in sweep().step_by(7) {
        for scheme in Scheme::ALL {
            let palette = Palette::new(scheme, accent);
            let [fill, hover, pressed] = [AccentRole::Fill, AccentRole::FillHover, AccentRole::FillPressed]
                .map(|role| palette.color(Role::Accent(role)));
            assert!(fill != hover && hover != pressed && fill != pressed, "{accent:?} in {scheme:?}");
            let [fill, hover, pressed] =
                [DestructiveRole::Fill, DestructiveRole::FillHover, DestructiveRole::FillPressed]
                    .map(|role| palette.color(Role::Destructive(role)));
            assert!(fill != hover && hover != pressed && fill != pressed, "destructive in {scheme:?}");
        }
    }
}

/// A role no pair mentions is a colour nobody checks. Only the grounds (the surfaces and the tints, what text is
/// read against) and the decorative divider stand outside the contract.
#[test]
fn every_role_but_the_grounds_and_the_divider_is_in_the_contract() {
    let pairs = contract();
    for role in Role::all() {
        let as_ground = pairs.iter().any(|pair| pair.ground == Ground::Role(role));
        let checked = pairs.iter().any(|pair| pair.role == role);
        match role {
            Role::Surface(_)
            | Role::Accent(AccentRole::Tint)
            | Role::Status(_, StatusRole::Tint)
            | Role::ScrimHover
            | Role::ScrimPressed
            | Role::DisabledFill
            | Role::PictureGround => {
                assert!(as_ground, "{role:?} is read against")
            }
            Role::Border(Border::Subtle) => assert!(!checked, "the divider is decoration"),
            _ => assert!(checked, "{role:?} has no requirement"),
        }
    }
}

/// A picked colour is drawn as picked, so only its letter answers for reading it: black or white,
/// whichever is better, and that is 4.5:1 on any colour, the sweep and the hostile ones included.
#[test]
fn the_letter_on_any_colour_is_the_better_of_black_and_white_and_reads() {
    let hostile = [0xFFFF00, 0xFF0000, 0x00FF00, 0x0000FF, 0x808080, 0x000000, 0x010101, 0xFFFFFF, 0xFEFEFE, 0x7F7F00]
        .map(Rgb::hex);
    let greys = (0..=255).map(|value| Rgb::new(value, value, value));
    for fill in sweep().map(Accent::seed).chain(hostile).chain(greys) {
        let letter = readable_on(fill);
        let other = if letter == Rgb::WHITE { Rgb::BLACK } else { Rgb::WHITE };
        let found = Ratio::between(letter, fill);
        assert!(found >= Ratio::between(other, fill), "{fill:?}: {letter:?} is the worse letter");
        assert!(found.meets(Ratio::TEXT), "{fill:?}: {found:?}");
    }
}

/// "More contrast" is stronger where it is read most: secondary text is 7:1 on every surface, and quieter than the
/// main text still.
#[test]
fn more_contrast_makes_secondary_text_seven_to_one_and_keeps_it_quieter() {
    for scheme in Scheme::ALL {
        let standard = Palette::new(scheme, Accent::DEFAULT);
        let more = Palette::with_contrast(scheme, Accent::DEFAULT, Contrast::More);
        for surface in Surface::ALL {
            let ground = more.color(Role::Surface(surface));
            let secondary = Ratio::between(more.color(Role::Content(Content::Secondary)), ground);
            let primary = Ratio::between(more.color(Role::Content(Content::Primary)), ground);
            assert!(secondary.meets(Ratio::TEXT_MORE), "{scheme:?} {surface:?}: {}", secondary.value());
            assert!(primary.value() > secondary.value(), "{scheme:?} {surface:?}: still quieter");
        }
        assert_ne!(
            standard.color(Role::Content(Content::Secondary)),
            more.color(Role::Content(Content::Secondary)),
            "{scheme:?}: the standard secondary text is under 7:1, so it moves"
        );
    }
}
