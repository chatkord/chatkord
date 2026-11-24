//! What a screen reader's requests come to: sent from the adapter's thread to the window's loop, resolved there against
//! the controls of the last tree.

use super::Heard;
use crate::asks::Asking;
use accesskit::{Action, ActionHandler, ActionRequest};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use ui::access::Controls;

/// What a screen reader asks a control to do: sent to the window's loop, which carries it out on its thread.
pub(super) struct Heeded {
    requests: Sender<ActionRequest>,
    asking: Arc<Asking>,
}

impl Heeded {
    /// The handler for a window `asking` serves, and the end its requests arrive at.
    pub(super) fn channel(asking: Arc<Asking>) -> (Heeded, Receiver<ActionRequest>) {
        let (requests, arrived) = mpsc::channel();
        (Heeded { requests, asking }, arrived)
    }
}

impl ActionHandler for Heeded {
    fn do_action(&mut self, request: ActionRequest) {
        if self.requests.send(request).is_ok() {
            self.asking.whole();
        }
    }
}

/// What a request comes to, by the controls of the last tree; a node the tree no longer has, or an action no control
/// offers, is none.
fn heard(request: &ActionRequest, controls: &Controls) -> Option<Heard> {
    let Some(control) = controls.get(&request.target_node) else {
        report::note!(debug, "a screen reader asked of a control the tree does not have");
        return None;
    };
    match request.action {
        Action::Focus => control.focus.map(Heard::Focus),
        Action::Click => Some(Heard::Press(control.bounds)),
        _ => {
            report::note!(debug, "a screen reader asked for an action that is not carried out");
            None
        }
    }
}

/// Everything the receiver holds, as what the loop does.
pub(super) fn drain(requests: &Receiver<ActionRequest>, controls: &Controls) -> Vec<Heard> {
    requests.try_iter().filter_map(|request| heard(&request, controls)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::press_at;
    use accesskit::{NodeId, TreeId};
    use iced_winit::core::{Event, Point, Rectangle, mouse};
    use ui::access::Control;

    const BUTTON: NodeId = NodeId(7);
    const GONE: NodeId = NodeId(8);

    fn controls() -> Controls {
        let bounds = Rectangle { x: 10.0, y: 20.0, width: 100.0, height: 40.0 };
        Controls::from([(BUTTON, Control { bounds, focus: Some(3) })])
    }

    fn asked(action: Action, node: NodeId) -> ActionRequest {
        ActionRequest { action, target_tree: TreeId::ROOT, target_node: node, data: None }
    }

    #[test]
    fn a_click_request_presses_the_control_at_its_centre() {
        let Some(Heard::Press(bounds)) = heard(&asked(Action::Click, BUTTON), &controls()) else {
            panic!("a click on a known control is a press");
        };
        assert_eq!(
            press_at(bounds),
            [
                Event::Mouse(mouse::Event::CursorMoved { position: Point::new(60.0, 40.0) }),
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            ]
        );
    }

    #[test]
    fn a_focus_request_for_a_known_node_focuses_its_place_and_others_are_ignored() {
        assert_eq!(heard(&asked(Action::Focus, BUTTON), &controls()), Some(Heard::Focus(3)));
        assert_eq!(heard(&asked(Action::Focus, GONE), &controls()), None);
        assert_eq!(heard(&asked(Action::ScrollIntoView, BUTTON), &controls()), None);
    }
}
