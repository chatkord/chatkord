//! FFmpeg's status codes as a type.

use ffmpeg_sys_next as ff;
use std::ffi::c_int;
use std::fmt;

const AGAIN: c_int = ff::AVERROR(ff::EAGAIN);
const NO_MEMORY: c_int = ff::AVERROR(ff::ENOMEM);
const INVALID: c_int = ff::AVERROR(ff::EINVAL);
const UNSUPPORTED: c_int = ff::AVERROR(ff::ENOSYS);

/// Why an FFmpeg call did not succeed, from the negative status it returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq, report::Public)]
pub enum AvError {
    /// `AVERROR(EAGAIN)`: the codec wants input, or has no output yet.
    Again,
    /// `AVERROR_EOF`: the stream has ended.
    End,
    /// `AVERROR(ENOMEM)`: no memory.
    NoMemory,
    /// `AVERROR(EINVAL)`: an argument FFmpeg refuses.
    Invalid,
    /// `AVERROR(ENOSYS)`: the build or the driver does not do this.
    Unsupported,
    /// Any other status, as FFmpeg returned it.
    Other(#[public] i32),
}

impl AvError {
    /// The error a negative `status` stands for.
    pub fn from_code(status: c_int) -> Self {
        match status {
            AGAIN => Self::Again,
            ff::AVERROR_EOF => Self::End,
            NO_MEMORY => Self::NoMemory,
            INVALID => Self::Invalid,
            UNSUPPORTED => Self::Unsupported,
            other => Self::Other(other),
        }
    }

    /// The status FFmpeg gives for it.
    #[cfg(test)]
    pub fn code(self) -> c_int {
        match self {
            Self::Again => AGAIN,
            Self::End => ff::AVERROR_EOF,
            Self::NoMemory => NO_MEMORY,
            Self::Invalid => INVALID,
            Self::Unsupported => UNSUPPORTED,
            Self::Other(code) => code,
        }
    }
}

/// `status` when it is not negative, else its error.
pub(super) fn check(status: c_int) -> Result<c_int, AvError> {
    if status < 0 { Err(AvError::from_code(status)) } else { Ok(status) }
}

impl fmt::Display for AvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Again => f.write_str("it wants more input or has no output yet"),
            Self::End => f.write_str("the stream ended"),
            Self::NoMemory => f.write_str("out of memory"),
            Self::Invalid => f.write_str("an argument was refused"),
            Self::Unsupported => f.write_str("this build or driver does not do it"),
            Self::Other(code) => write!(f, "status {code}"),
        }
    }
}

impl std::error::Error for AvError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_status_is_the_error_it_names_and_back() {
        let cases = [
            (ff::AVERROR(ff::EAGAIN), AvError::Again),
            (ff::AVERROR_EOF, AvError::End),
            (ff::AVERROR(ff::ENOMEM), AvError::NoMemory),
            (ff::AVERROR(ff::EINVAL), AvError::Invalid),
            (ff::AVERROR(ff::ENOSYS), AvError::Unsupported),
            (-12345, AvError::Other(-12345)),
        ];
        for (status, error) in cases {
            assert_eq!(AvError::from_code(status), error, "{status}");
            assert_eq!(error.code(), status, "{error:?}");
        }
    }
}
