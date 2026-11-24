//! Every look a button or a text field can have, in every scheme and under every accent, meets the contrast the
//! contract names for what it is: its text on its fill (or on any surface, where it has none), and
//! its edge on any surface it sits on. A new look is covered by these loops without a line here.

use design::{
    Accent, ButtonLook, ButtonVariant, Ground, InputState, Interaction, Oklch, Pair, Palette, PillKind, Presence,
    Ratio, Role, Scheme, Surface, badge, button, danger_row, input, pill, plain_avatar, quiet_row, row,
};

fn accents() -> impl Iterator<Item = Accent> {
    (0..360).step_by(10).flat_map(|hue| {
        [(0.0, 0.5), (0.15, 0.6), (0.3, 0.95), (0.15, 0.2)]
            .map(move |(chroma, l)| Accent::from(Oklch { l, c: chroma, h: f64::from(hue) }.to_rgb()))
    })
}

/// What a role is painted over: the fill the look names, else the scrim for a control on a picture's bar, the picture
/// ground for one on the ground, else any surface the control can sit on.
fn grounds(variant: ButtonVariant, look: &ButtonLook) -> Vec<Ground> {
    match (look.fill, variant) {
        (Some(fill), _) => vec![Ground::Role(fill)],
        (None, ButtonVariant::OnScrim) => vec![Ground::Scrim],
        (None, ButtonVariant::OnPicture) => vec![Ground::Role(Role::PictureGround)],
        (None, _) => Surface::ALL.map(|surface| Ground::Role(Role::Surface(surface))).to_vec(),
    }
}

#[test]
fn every_button_look_meets_the_contrast_its_parts_must() {
    for accent in accents() {
        for scheme in Scheme::ALL {
            let palette = Palette::new(scheme, accent);
            for variant in ButtonVariant::ALL {
                for interaction in Interaction::ALL {
                    let look = button(variant, interaction);
                    let minimum = if interaction == Interaction::Disabled { Ratio::DISABLED } else { Ratio::TEXT };
                    for ground in grounds(variant, &look) {
                        let found = Pair { role: look.text, ground, min: minimum }.measure(&palette);
                        assert!(
                            found.meets(minimum),
                            "{variant:?}/{interaction:?} text on {ground:?} in {scheme:?} under {accent:?}: {found:?}"
                        );
                    }
                    // The edge is found against what the control sits on: the picture's ground, else any surface.
                    if let Some(edge) = look.border {
                        let grounds: Vec<Role> = match variant {
                            ButtonVariant::OnPicture => vec![Role::PictureGround],
                            _ => Surface::ALL.map(Role::Surface).to_vec(),
                        };
                        for ground in grounds {
                            let found = Ratio::between(palette.color(edge), palette.color(ground));
                            assert!(
                                found.meets(Ratio::NON_TEXT),
                                "{variant:?}/{interaction:?} edge on {ground:?}: {found:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}

/// A person who cannot tell two hues apart still sees the control answer the pointer: idle,
/// hovered and pressed are three different looks, and a disabled one is none of them.
#[test]
fn a_button_answers_the_pointer_with_a_visible_change() {
    for variant in ButtonVariant::ALL {
        let [idle, hover, pressed, disabled] =
            [Interaction::Idle, Interaction::Hover, Interaction::Pressed, Interaction::Disabled]
                .map(|i| button(variant, i));
        assert!(idle != hover && hover != pressed && idle != pressed, "{variant:?}");
        assert!(disabled != idle && disabled != hover && disabled != pressed, "{variant:?}");
        assert_eq!(
            button(variant, Interaction::Loading),
            idle,
            "{variant:?}: loading looks as it did and takes no input"
        );
    }
}

/// A text field's value and hint are text, its edge must be found, and a field that is only
/// disabled need not be: every state, valid or not, in every scheme and under every accent.
#[test]
fn every_text_field_look_meets_the_contrast_its_parts_must() {
    for accent in accents() {
        for scheme in Scheme::ALL {
            let palette = Palette::new(scheme, accent);
            for state in InputState::ALL {
                for invalid in [false, true] {
                    let look = input(state, invalid);
                    let disabled = state == InputState::Disabled;
                    let text = if disabled { Ratio::DISABLED } else { Ratio::TEXT };
                    let fill = palette.color(look.fill);
                    for (part, role) in [("text", look.text), ("placeholder", look.placeholder)] {
                        let found = Ratio::between(palette.color(role), fill);
                        assert!(
                            found.meets(text),
                            "{part} of {state:?}/{invalid} in {scheme:?} under {accent:?}: {found:?}"
                        );
                    }
                    if !disabled {
                        // The edge is what finds the field: against its own fill and the surface around it.
                        for ground in [look.fill].into_iter().chain(Surface::ALL.map(Role::Surface)) {
                            let found = Ratio::between(palette.color(look.border), palette.color(ground));
                            assert!(
                                found.meets(Ratio::NON_TEXT),
                                "edge of {state:?}/{invalid} on {ground:?} in {scheme:?}: {found:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}

/// Focus and a fault are each seen without telling hues apart: the focused edge is thicker than the
/// idle one, and a field's states are not one look.
#[test]
fn a_text_field_shows_focus_by_width_as_well_as_colour() {
    for invalid in [false, true] {
        assert!(input(InputState::Focused, invalid).border_width > input(InputState::Idle, invalid).border_width);
    }
    assert_ne!(input(InputState::Idle, false), input(InputState::Hover, false));
    assert_ne!(input(InputState::Idle, false), input(InputState::Idle, true));
    assert_ne!(input(InputState::Idle, false), input(InputState::Disabled, false));
}

/// The answer to the pointer is seen whatever the control sits on: a hovered or pressed fill is told apart
/// from every surface, and from each other.
#[test]
fn a_hover_and_a_pressed_fill_are_visibly_different_from_every_surface_under_them() {
    for scheme in Scheme::ALL {
        let palette = Palette::new(scheme, Accent::DEFAULT);
        let hover = palette.color(Role::Surface(Surface::Hover));
        let pressed = palette.color(Role::Surface(Surface::Pressed));
        for ground in Surface::GROUNDS {
            let under = palette.color(Role::Surface(ground));
            for (name, fill) in [("hover", hover), ("pressed", pressed)] {
                let found = Ratio::between(fill, under);
                assert!(found.meets(Ratio::VISIBLE), "{name} on {ground:?} in {scheme:?}: {found:?}");
            }
        }
        assert!(Ratio::between(hover, pressed).meets(Ratio::VISIBLE), "hover and pressed in {scheme:?}");
    }
}

/// A picture with no colour of its own is a disc on whatever it sits on: every ground (a card on the inset surface
/// too), the chrome of the account strip and a row under the pointer; and its initials are read on it.
#[test]
fn a_plain_avatar_is_a_visible_disc_on_every_surface_with_readable_initials() {
    let under = Surface::GROUNDS.into_iter().chain([Surface::Chrome, Surface::Hover]);
    for scheme in Scheme::ALL {
        let palette = Palette::new(scheme, Accent::DEFAULT);
        let look = plain_avatar();
        let fill = palette.color(look.fill);
        for ground in under.clone() {
            let found = Ratio::between(fill, palette.color(Role::Surface(ground)));
            assert!(found.meets(Ratio::VISIBLE), "the disc on {ground:?} in {scheme:?}: {found:?}");
        }
        let found = Ratio::between(palette.color(look.text), fill);
        assert!(found.meets(Ratio::TEXT), "its initials in {scheme:?}: {found:?}");
    }
}

/// A row's text is readable on its fill (or on any surface, with none), its mark is found, and a selected row
/// is told from an unselected one by weight and a mark as well as by its fill.
#[test]
fn every_row_look_meets_the_contrast_its_parts_must() {
    for accent in accents() {
        for scheme in Scheme::ALL {
            let palette = Palette::new(scheme, accent);
            for selected in [false, true] {
                for interaction in Interaction::ALL {
                    let pills = PillKind::ALL.map(|kind| pill(kind, selected, interaction));
                    let rows = [row(selected, interaction), quiet_row(selected, interaction), danger_row(interaction)];
                    for look in rows.into_iter().chain(pills) {
                        let minimum = if interaction == Interaction::Disabled { Ratio::DISABLED } else { Ratio::TEXT };
                        let grounds =
                            look.fill.map_or_else(|| Surface::ALL.map(Role::Surface).to_vec(), |fill| vec![fill]);
                        for ground in grounds {
                            let found = Ratio::between(palette.color(look.text), palette.color(ground));
                            assert!(
                                found.meets(minimum),
                                "row {selected}/{interaction:?} text on {ground:?} in {scheme:?}: {found:?}"
                            );
                        }
                    }
                }
            }
        }
    }
    let (plain, chosen) = (row(false, Interaction::Idle), row(true, Interaction::Idle));
    assert!(chosen.strong && !plain.strong && chosen.fill.is_some() && plain.fill.is_none());
    for kind in PillKind::ALL {
        let (plain, chosen) = (pill(kind, false, Interaction::Idle), pill(kind, true, Interaction::Idle));
        assert!(chosen.strong && !plain.strong && chosen.fill.is_some() && plain.fill.is_none(), "{kind:?}");
    }
}

/// A badge's word is read on its colour, and the colour is found against what the badge sits on.
#[test]
fn every_badge_meets_the_contrast_its_parts_must() {
    for accent in accents() {
        for scheme in Scheme::ALL {
            let palette = Palette::new(scheme, accent);
            for presence in Presence::ALL {
                let look = badge(presence);
                let found = Ratio::between(palette.color(look.text), palette.color(look.fill));
                assert!(found.meets(Ratio::TEXT), "{presence:?} word in {scheme:?}: {found:?}");
                for surface in Surface::GROUNDS {
                    let found = Ratio::between(palette.color(look.fill), palette.color(Role::Surface(surface)));
                    assert!(found.meets(Ratio::NON_TEXT), "{presence:?} on {surface:?} in {scheme:?}: {found:?}");
                }
            }
        }
    }
}

#[test]
fn a_switchs_knob_is_found_on_its_track_and_its_track_on_every_ground_in_both_states() {
    use design::{SWITCH_HEIGHT, SWITCH_KNOB, SWITCH_PAD, SWITCH_WIDTH, Surface, switch};
    for scheme in Scheme::ALL {
        for accent in [Accent::DEFAULT, Accent::from(Oklch { l: 0.5, c: 0.15, h: 250.0 }.to_rgb())] {
            let palette = Palette::new(scheme, accent);
            for (on, interaction) in [false, true].into_iter().flat_map(|on| Interaction::ALL.map(|i| (on, i))) {
                let look = switch(on, interaction);
                let found = Ratio::between(palette.color(look.knob), palette.color(look.track));
                assert!(
                    found.meets(Ratio::NON_TEXT),
                    "the knob on the track, on={on} {interaction:?}, {scheme:?}: {found:?}"
                );
                for ground in Surface::GROUNDS {
                    let found = Ratio::between(palette.color(look.track), palette.color(Role::Surface(ground)));
                    assert!(
                        found.meets(Ratio::NON_TEXT),
                        "the track on {ground:?}, on={on} {interaction:?}, {scheme:?}: {found:?}"
                    );
                }
            }
        }
    }
    assert_ne!(switch(true, Interaction::Idle), switch(false, Interaction::Idle));
    assert_eq!(SWITCH_KNOB + 2.0 * SWITCH_PAD, SWITCH_HEIGHT, "the knob fills the track's height but its room");
    const { assert!(SWITCH_WIDTH >= 2.0 * SWITCH_KNOB, "the knob has somewhere to go") };
}

/// A switch that cannot be taken is drawn at the disabled share, on or off, so one held on still reads as on and as
/// out of reach; any other state shows it whole.
#[test]
fn a_switch_that_cannot_be_taken_shows_its_state_dimmed() {
    use design::{DISABLED_ALPHA, switch};
    for on in [false, true] {
        for interaction in Interaction::ALL {
            let alpha = switch(on, interaction).alpha;
            match interaction {
                Interaction::Disabled => assert_eq!(alpha, DISABLED_ALPHA, "on={on}"),
                Interaction::Idle | Interaction::Hover | Interaction::Pressed | Interaction::Loading => {
                    assert_eq!(alpha, u8::MAX, "on={on} {interaction:?}")
                }
            }
        }
        let (dimmed, idle) = (switch(on, Interaction::Disabled), switch(on, Interaction::Idle));
        assert_eq!((dimmed.track, dimmed.knob), (idle.track, idle.knob), "on={on}: dimmed, not recoloured");
    }
}

/// Keyboard focus is shown by a control's hover look alone, so every control that can be focused looks
/// different hovered than at rest; one that did not would show no focus at all.
#[test]
fn every_control_looks_different_hovered() {
    use design::{CallControl, Interaction::*, PillKind};
    for variant in ButtonVariant::ALL {
        assert_ne!(design::button(variant, Idle), design::button(variant, Hover), "{variant:?}");
    }
    for control in CallControl::ALL {
        assert_ne!(design::call_button(control, Idle), design::call_button(control, Hover), "{control:?}");
    }
    for selected in [false, true] {
        for kind in PillKind::ALL {
            assert_ne!(design::pill(kind, selected, Idle), design::pill(kind, selected, Hover), "{kind:?} {selected}");
        }
        assert_ne!(design::row(selected, Idle), design::row(selected, Hover), "row {selected}");
        assert_ne!(design::quiet_row(selected, Idle), design::quiet_row(selected, Hover), "quiet row {selected}");
        assert_ne!(design::switch(selected, Idle), design::switch(selected, Hover), "switch {selected}");
    }
    assert_ne!(design::danger_row(Idle), design::danger_row(Hover));
}
