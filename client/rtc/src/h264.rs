//! Whether two ends mean the same H.264, matched as libwebrtc and mediasoup's own client do. An absent parameter means
//! its RFC 6184 default as libwebrtc reads it, on both sides, so silence is not a mismatch.

/// The profile a `profile-level-id` names, for the profiles an encoder here can write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Profile {
    ConstrainedBaseline,
    Baseline,
    Main,
    High,
    ConstrainedHigh,
}

/// How `profile_idc` and the constraint bits (`profile-iop`) name a profile: `(idc, mask, bits)` matches when
/// `iop & mask == bits`. The bits a profile does not care about are outside its mask.
const PROFILES: [(u8, u8, u8, Profile); 8] = [
    (0x42, 0x4F, 0x40, Profile::ConstrainedBaseline),
    (0x4D, 0x8F, 0x80, Profile::ConstrainedBaseline),
    (0x58, 0xCF, 0xC0, Profile::ConstrainedBaseline),
    (0x42, 0x4F, 0x00, Profile::Baseline),
    (0x58, 0xCF, 0x80, Profile::Baseline),
    (0x4D, 0xAF, 0x00, Profile::Main),
    (0x64, 0xFF, 0x00, Profile::High),
    (0x64, 0xFF, 0x0C, Profile::ConstrainedHigh),
];

/// Constrained Baseline at level 3.1, what a missing `profile-level-id` means.
const DEFAULT_PROFILE_LEVEL: (Profile, u8) = (Profile::ConstrainedBaseline, 0x1F);

/// The parameters of an H.264 codec that decide whether two ends can talk.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct H264 {
    packetization_mode: u8,
    profile: Profile,
    level: u8,
    level_asymmetry_allowed: bool,
}

impl H264 {
    /// Reads the parameters through `get`, which gives a parameter's text. A `profile-level-id` that names no profile
    /// read here is `None`: a codec that cannot be matched, which is not silence.
    pub(crate) fn read(get: impl Fn(&str) -> Option<String>) -> Option<Self> {
        let (profile, level) = match get("profile-level-id") {
            Some(id) => profile_and_level(&id)?,
            None => DEFAULT_PROFILE_LEVEL,
        };
        Some(Self {
            packetization_mode: get("packetization-mode").map_or(Some(0), |mode| mode.parse().ok())?,
            profile,
            level,
            level_asymmetry_allowed: get("level-asymmetry-allowed").is_some_and(|allowed| allowed == "1"),
        })
    }

    /// Whether this end and the router can use one another's stream.
    pub(crate) fn agrees_with(&self, router: &Self) -> bool {
        self.packetization_mode == router.packetization_mode
            && self.profile == router.profile
            && (self.level == router.level || (self.level_asymmetry_allowed && router.level_asymmetry_allowed))
    }
}

fn profile_and_level(id: &str) -> Option<(Profile, u8)> {
    if id.len() != 6 {
        return None;
    }
    let byte = |at: usize| id.get(at..at + 2).and_then(|pair| u8::from_str_radix(pair, 16).ok());
    let (idc, iop, level) = (byte(0)?, byte(2)?, byte(4)?);
    PROFILES
        .iter()
        .find(|(known, mask, bits, _)| *known == idc && iop & mask == *bits)
        .map(|(.., profile)| (*profile, level))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(pairs: &[(&str, &str)]) -> Option<H264> {
        H264::read(|key| pairs.iter().find(|(name, _)| *name == key).map(|(_, value)| (*value).to_string()))
    }

    #[test]
    fn the_five_profiles_are_told_apart_by_their_constraint_bits() {
        let profile = |id| with(&[("profile-level-id", id)]).map(|h264| h264.profile);
        assert_eq!(profile("42e01f"), Some(Profile::ConstrainedBaseline));
        assert_eq!(profile("42001f"), Some(Profile::Baseline));
        assert_eq!(profile("4d001f"), Some(Profile::Main));
        assert_eq!(profile("64001f"), Some(Profile::High));
        assert_eq!(profile("640c1f"), Some(Profile::ConstrainedHigh));
        assert_eq!(profile("4d801f"), Some(Profile::ConstrainedBaseline));
    }

    #[test]
    fn what_names_no_profile_is_refused_not_defaulted() {
        for id in ["", "zz0000", "42e01", "42e01f0", "ffffff", "42e0é1"] {
            assert_eq!(with(&[("profile-level-id", id)]), None, "{id:?}");
        }
        assert_eq!(with(&[("packetization-mode", "x")]), None);
    }
}
