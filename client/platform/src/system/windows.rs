//! Windows: the product name and version numbers in the registry's `CurrentVersion` key, which unlike
//! `GetVersionEx` do not depend on the program's manifest.

use super::{Session, System};
use report::{Fact, Product};
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW};

const KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";

pub(super) fn read() -> System {
    let numbers =
        (number("CurrentMajorVersionNumber"), number("CurrentMinorVersionNumber"), text("CurrentBuildNumber"));
    let build = numbers.2.as_deref().and_then(|build| build.parse().ok());
    let name = text("ProductName").map_or(Fact::Absent, |name| Product::fact_of(&named(&name, build)));
    let version = match numbers {
        (Some(major), Some(minor), Some(build)) => {
            let revision = number("UBR").map_or_else(String::new, |revision| format!(".{revision}"));
            Product::fact_of(&format!("{major}.{minor}.{build}{revision}"))
        }
        _ => Fact::Absent,
    };
    System { name, version, session: Session::Windows }
}

/// The product's name as the release it is: the registry still names every Windows 11 "Windows 10", which its
/// build number, 22000 and later, tells apart.
fn named(product: &str, build: Option<u32>) -> String {
    match product.strip_prefix("Windows 10") {
        Some(edition) if build.is_some_and(|build| build >= FIRST_WINDOWS_11) => format!("Windows 11{edition}"),
        _ => product.to_owned(),
    }
}

/// Windows 11's first build.
const FIRST_WINDOWS_11: u32 = 22_000;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// A whole-number value of the key.
fn number(name: &str) -> Option<u32> {
    let (key, name) = (wide(KEY), wide(name));
    let mut value = 0u32;
    let mut size = 4u32;
    // SAFETY: NUL-terminated names alive across the call, and a buffer of the four bytes a number takes, as size states.
    let read = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&raw mut value).cast(),
            &raw mut size,
        )
    };
    (read == ERROR_SUCCESS).then_some(value)
}

/// A text value of the key, at most 127 characters.
fn text(name: &str) -> Option<String> {
    let (key, name) = (wide(KEY), wide(name));
    let mut buffer = [0u16; 128];
    let mut size = u32::try_from(std::mem::size_of_val(&buffer)).ok()?;
    // SAFETY: NUL-terminated names alive across the call, and a buffer of the byte size stated, which the call
    // terminates.
    let read = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &raw mut size,
        )
    };
    if read != ERROR_SUCCESS {
        return None;
    }
    let end = buffer.iter().position(|unit| *unit == 0).unwrap_or(buffer.len());
    String::from_utf16(&buffer[..end]).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_11_is_named_by_its_build_where_the_registry_says_windows_10() {
        assert_eq!(named("Windows 10 Pro", Some(26_300)), "Windows 11 Pro");
        assert_eq!(named("Windows 10 Pro", Some(19_045)), "Windows 10 Pro");
        assert_eq!(named("Windows 10 Pro", None), "Windows 10 Pro");
        assert_eq!(named("Windows Server 2022 Standard", Some(26_300)), "Windows Server 2022 Standard");
    }
}
