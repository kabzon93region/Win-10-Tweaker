//! Update check through the public GitHub Releases API.
//!
//! Only the version tag is read; nothing is downloaded or installed. The check
//! uses HTTPS and the system certificate store. A signed manifest and automatic
//! installation are not implemented yet (D-53).

use std::os::windows::process::CommandExt;
use std::process::Command;

use tweaker_core::adapters::AdapterError;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Latest release tag of `owner/repo`, or `Ok(None)` when there are no releases.
pub fn latest_tag(repo: &str) -> Result<Option<String>, AdapterError> {
    let script = format!(
        "try{{$r=Invoke-RestMethod -Headers @{{'User-Agent'='WinTweaker'}} -Uri 'https://api.github.com/repos/{repo}/releases/latest' -UseBasicParsing; $r.tag_name}}catch{{if($_.Exception.Response.StatusCode.value__ -eq 404){{''}}else{{throw}}}}"
    );
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| AdapterError::new(format!("cannot run PowerShell: {error}")))?;
    if !output.status.success() {
        return Err(AdapterError::new(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    let tag = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    Ok(normalize_tag(&tag))
}

/// Trims the tag and removes a leading `v`.
pub fn normalize_tag(tag: &str) -> Option<String> {
    let tag = tag.trim();
    if tag.is_empty() {
        return None;
    }
    Some(tag.trim_start_matches(['v', 'V']).to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_are_normalized() {
        assert_eq!(normalize_tag("v0.2.0"), Some("0.2.0".to_owned()));
        assert_eq!(normalize_tag(" 0.2.0 "), Some("0.2.0".to_owned()));
        assert_eq!(normalize_tag(""), None);
    }
}
