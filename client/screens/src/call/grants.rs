//! What this account may do in a call and to those in it, as its standing says.

use super::*;

impl Place {
    /// What this account may do in the channel of the call held here, as its standing there says: nothing with none.
    fn call_grants(&self) -> Grants {
        self.call().map_or(Grants::NONE, |call| self.grants_in(&call.channel.id))
    }

    /// Whether this account can send its screen in the call here, and if not why.
    pub fn can_share(&self) -> Can {
        can_share(self.call_grants(), self.encodes)
    }

    /// Whether this account can turn on its camera in the call here, and if not why.
    pub fn can_camera(&self) -> CanCamera {
        if self.call_grants().holds(Grant::ShareCamera) { CanCamera::Yes } else { CanCamera::NotAllowed }
    }

    /// Whether this account hears `user_id` in its call: someone else, in the call it is in now. Their tile's menu then
    /// offers their volume, and their screen's sound its own.
    pub fn hears(&self, user_id: &MemberId) -> bool {
        let Some(standing) = &self.standing else { return false };
        matches!(&self.status, Status::Signed(signed) if may_open(standing, signed, user_id))
    }

    /// Whether this account may act on `user_id` in a call as an administrator, in that call or not: their popover
    /// offers it.
    pub fn may_moderate(&self, user_id: &MemberId) -> bool {
        let Some(standing) = &self.standing else { return false };
        matches!(&self.status, Status::Signed(signed) if may_moderate(standing, signed, user_id))
    }

    /// Whether `user_id` is muted for everyone, as the server last said of the call they are in; none outside every
    /// call.
    pub fn held(&self, user_id: &MemberId) -> Option<bool> {
        let Status::Signed(signed) = &self.status else { return None };
        occupant(signed, user_id).map(|occupant| occupant.held)
    }
}

/// Whether an account holding `grants` can send its screen from a device that does or does not `encodes` one.
fn can_share(grants: Grants, encodes: bool) -> Can {
    if !grants.holds(Grant::ShareScreen) {
        Can::NotAllowed
    } else if !encodes {
        Can::NoEncoder
    } else {
        Can::Yes
    }
}

/// Whether `user_id`'s popover may open in the call this account is in: someone else, in the call now.
pub(super) fn may_open(standing: &Standing, signed: &SignedIn, user_id: &MemberId) -> bool {
    signed.call.as_ref().filter(|call| call.live() && call.joined()).is_some_and(|call| {
        standing.user_id != *user_id && call.tiles().iter().any(|tile| tile.user_id.as_ref() == Some(user_id))
    })
}

/// Who `user_id` is in the voice room the server last said they are in; none outside every room.
fn occupant<'a>(signed: &'a SignedIn, user_id: &MemberId) -> Option<&'a Occupant> {
    signed.rooms.values().flatten().find(|occupant| occupant.user_id == *user_id)
}

/// Whether this account may moderate `user_id` in a call, in that call or not: under `MODERATE_VOICE`, never itself,
/// someone the server lists in a voice room, and no one the server would refuse: only a person placed below it in the
/// roles' order (one the server lists with no place is at the bottom).
fn may_moderate(standing: &Standing, signed: &SignedIn, user_id: &MemberId) -> bool {
    if !standing.grants.holds(Grant::ModerateVoice) || standing.user_id == *user_id {
        return false;
    }
    occupant(signed, user_id).is_some_and(|occupant| standing.place.outranks(occupant.place))
}
