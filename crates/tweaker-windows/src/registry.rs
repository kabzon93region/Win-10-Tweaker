//! Real registry adapter on top of `winreg`.

use std::borrow::Cow;
use std::io;

use tweaker_core::action::{RegistryKey, RegistryRoot, RegistryValue};
use tweaker_core::adapters::{AdapterError, IRegistry};
use winreg::RegKey;
use winreg::RegValue;
use winreg::enums::{
    HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, HKEY_USERS, KEY_SET_VALUE, RegType,
};

#[cfg(test)]
use winreg::enums::KEY_ALL_ACCESS;

/// Registry access through the Windows API.
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsRegistry;

impl IRegistry for WindowsRegistry {
    fn read(&self, key: &RegistryKey, name: &str) -> Result<Option<RegistryValue>, AdapterError> {
        let root = predef(key.root);
        let subkey = match root.open_subkey(&key.path) {
            Ok(subkey) => subkey,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(AdapterError::new(format!(
                    "{key}: cannot open key: {error}"
                )));
            }
        };
        match subkey.get_raw_value(name) {
            Ok(raw) => decode(&raw).map(Some),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(AdapterError::new(format!(
                "{key}: cannot read value {name}: {error}"
            ))),
        }
    }

    fn write(
        &self,
        key: &RegistryKey,
        name: &str,
        value: &RegistryValue,
    ) -> Result<(), AdapterError> {
        let root = predef(key.root);
        let (subkey, _) = root
            .create_subkey(&key.path)
            .map_err(|error| AdapterError::new(format!("{key}: cannot create key: {error}")))?;
        let raw = encode(value);
        subkey
            .set_raw_value(name, &raw)
            .map_err(|error| AdapterError::new(format!("{key}: cannot write {name}: {error}")))
    }

    fn delete(&self, key: &RegistryKey, name: &str) -> Result<(), AdapterError> {
        let root = predef(key.root);
        let subkey = match root.open_subkey_with_flags(&key.path, KEY_SET_VALUE) {
            Ok(subkey) => subkey,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(AdapterError::new(format!(
                    "{key}: cannot open key for delete: {error}"
                )));
            }
        };
        match subkey.delete_value(name) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(AdapterError::new(format!(
                "{key}: cannot delete value {name}: {error}"
            ))),
        }
    }
}

fn predef(root: RegistryRoot) -> RegKey {
    let hkey = match root {
        RegistryRoot::Hkcu => HKEY_CURRENT_USER,
        RegistryRoot::Hklm => HKEY_LOCAL_MACHINE,
        RegistryRoot::Hkcr => HKEY_CLASSES_ROOT,
        RegistryRoot::Hku => HKEY_USERS,
    };
    RegKey::predef(hkey)
}

fn encode(value: &RegistryValue) -> RegValue<'static> {
    match value {
        RegistryValue::Dword(value) => RegValue {
            bytes: Cow::Owned(value.to_le_bytes().to_vec()),
            vtype: RegType::REG_DWORD,
        },
        RegistryValue::Qword(value) => RegValue {
            bytes: Cow::Owned(value.to_le_bytes().to_vec()),
            vtype: RegType::REG_QWORD,
        },
        RegistryValue::String(text) => RegValue {
            bytes: Cow::Owned(utf16_bytes(text)),
            vtype: RegType::REG_SZ,
        },
        RegistryValue::ExpandString(text) => RegValue {
            bytes: Cow::Owned(utf16_bytes(text)),
            vtype: RegType::REG_EXPAND_SZ,
        },
        RegistryValue::MultiString(items) => {
            let mut text = String::new();
            for item in items {
                text.push_str(item);
                text.push('\0');
            }
            text.push('\0');
            RegValue {
                bytes: Cow::Owned(utf16_bytes(&text)),
                vtype: RegType::REG_MULTI_SZ,
            }
        }
        RegistryValue::Binary(bytes) => RegValue {
            bytes: Cow::Owned(bytes.clone()),
            vtype: RegType::REG_BINARY,
        },
    }
}

fn decode(raw: &RegValue) -> Result<RegistryValue, AdapterError> {
    let bytes = raw.bytes.as_ref();
    match &raw.vtype {
        RegType::REG_DWORD => {
            let array: [u8; 4] = bytes.try_into().map_err(|_| {
                AdapterError::new(format!("REG_DWORD has {} bytes, expected 4", bytes.len()))
            })?;
            Ok(RegistryValue::Dword(u32::from_le_bytes(array)))
        }
        RegType::REG_QWORD => {
            let array: [u8; 8] = bytes.try_into().map_err(|_| {
                AdapterError::new(format!("REG_QWORD has {} bytes, expected 8", bytes.len()))
            })?;
            Ok(RegistryValue::Qword(u64::from_le_bytes(array)))
        }
        RegType::REG_SZ => Ok(RegistryValue::String(
            decode_utf16(bytes)?.trim_end_matches('\0').to_owned(),
        )),
        RegType::REG_EXPAND_SZ => Ok(RegistryValue::ExpandString(
            decode_utf16(bytes)?.trim_end_matches('\0').to_owned(),
        )),
        RegType::REG_MULTI_SZ => {
            let text = decode_utf16(bytes)?;
            let items = text
                .split('\0')
                .filter(|item| !item.is_empty())
                .map(str::to_owned)
                .collect();
            Ok(RegistryValue::MultiString(items))
        }
        RegType::REG_BINARY => Ok(RegistryValue::Binary(bytes.to_vec())),
        other => Err(AdapterError::new(format!(
            "unsupported registry type {other:?}"
        ))),
    }
}

fn utf16_bytes(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<u8>>()
}

fn decode_utf16(bytes: &[u8]) -> Result<String, AdapterError> {
    if !bytes.len().is_multiple_of(2) {
        return Err(AdapterError::new(format!(
            "UTF-16 value has an odd number of bytes: {}",
            bytes.len()
        )));
    }
    let (chunks, _) = bytes.as_chunks::<2>();
    let units: Vec<u16> = chunks
        .iter()
        .map(|chunk| u16::from_le_bytes(*chunk))
        .collect();
    String::from_utf16(&units).map_err(|error| AdapterError::new(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;

    fn round_trip(value: RegistryValue) {
        let raw = encode(&value);
        let decoded = decode(&raw).expect("decode");
        assert_eq!(decoded, value);
    }

    #[test]
    fn all_value_types_round_trip() {
        round_trip(RegistryValue::Dword(0x0102_0304));
        round_trip(RegistryValue::Qword(0x0102_0304_0506_0708));
        round_trip(RegistryValue::String("Привет".to_owned()));
        round_trip(RegistryValue::ExpandString(
            "%SystemRoot%\\Fonts".to_owned(),
        ));
        round_trip(RegistryValue::MultiString(vec![
            "one".to_owned(),
            "two".to_owned(),
        ]));
        round_trip(RegistryValue::Binary(vec![0, 1, 2, 255]));
    }

    #[test]
    fn reads_a_real_machine_value() {
        let registry = WindowsRegistry;
        let key = RegistryKey::new(
            RegistryRoot::Hklm,
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
        );
        let value = registry.read(&key, "CurrentBuildNumber").expect("read");
        match value {
            Some(RegistryValue::String(text)) => assert!(!text.is_empty()),
            other => panic!("unexpected value: {other:?}"),
        }
    }

    #[test]
    #[ignore = "registry writes are denied to processes started from the project folder; run this test from a copy outside it"]
    fn write_read_delete_under_current_user() {
        let registry = WindowsRegistry;
        let key = RegistryKey::new(RegistryRoot::Hkcu, r"Software\WinTweakerTests\unit-test");
        registry
            .write(&key, "Value", &RegistryValue::Dword(42))
            .expect("write");
        assert_eq!(
            registry.read(&key, "Value").expect("read"),
            Some(RegistryValue::Dword(42))
        );
        registry.delete(&key, "Value").expect("delete");
        assert_eq!(registry.read(&key, "Value").expect("read"), None);
        // The value is gone; deleting again must stay silent.
        registry.delete(&key, "Value").expect("second delete");
        cleanup();
    }

    fn cleanup() {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(tests) = hkcu.open_subkey_with_flags(r"Software\WinTweakerTests", KEY_ALL_ACCESS)
        {
            let _ = tests.delete_subkey("unit-test");
        }
        if let Ok(software) = hkcu.open_subkey_with_flags("Software", KEY_ALL_ACCESS) {
            let _ = software.delete_subkey("WinTweakerTests");
        }
    }

    #[test]
    fn missing_value_and_missing_key_read_as_none() {
        let registry = WindowsRegistry;
        let key = RegistryKey::new(RegistryRoot::Hkcu, r"Software\WinTweakerTests\no-such-key");
        assert_eq!(registry.read(&key, "Value").expect("read"), None);
    }

    #[test]
    fn odd_utf16_is_rejected() {
        let raw = RegValue {
            bytes: Cow::Owned(vec![0x41, 0x00, 0x42]),
            vtype: RegType::REG_SZ,
        };
        assert!(decode(&raw).is_err());
    }
}
