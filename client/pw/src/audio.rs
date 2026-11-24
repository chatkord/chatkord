//! The raw audio format a stream of this client offers, as the pod PipeWire reads it.

use std::io::Cursor;

use libspa as spa;
use libspa_sys as spa_sys;
use spa::param::ParamType;
use spa::param::audio::{AudioFormat, AudioInfoRaw};
use spa::pod::serialize::PodSerializer;
use spa::pod::{Object, Value};
use spa::utils::SpaTypes;

use crate::session::{Error, aligned};

/// `format` at `rate`, one channel for each of `positions`, as an aligned pod.
pub(crate) fn raw(format: AudioFormat, rate: u32, positions: &[u32]) -> Result<Vec<u64>, Error> {
    let mut info = AudioInfoRaw::new();
    info.set_format(format);
    info.set_rate(rate);
    info.set_channels(u32::try_from(positions.len()).map_err(|_| Error::Pod)?);
    let mut position = [0; spa_sys::SPA_AUDIO_MAX_CHANNELS as usize];
    position.get_mut(..positions.len()).ok_or(Error::Pod)?.copy_from_slice(positions);
    info.set_position(position);
    let object = Object {
        type_: SpaTypes::ObjectParamFormat.as_raw(),
        id: ParamType::EnumFormat.as_raw(),
        properties: info.into(),
    };
    let bytes = PodSerializer::serialize(Cursor::new(Vec::new()), &Value::Object(object))
        .map(|(bytes, _)| bytes.into_inner())
        .map_err(|_| Error::Pod)?;
    aligned(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_format_is_a_whole_pod_and_too_many_channels_are_refused() {
        assert!(raw(AudioFormat::F32LE, 48_000, &[spa_sys::SPA_AUDIO_CHANNEL_MONO]).is_ok());
        let many = vec![spa_sys::SPA_AUDIO_CHANNEL_FL; spa_sys::SPA_AUDIO_MAX_CHANNELS as usize + 1];
        assert!(matches!(raw(AudioFormat::F32LE, 48_000, &many), Err(Error::Pod)));
    }
}
