//! Secure per-provider API key storage backed by Windows Credential Manager.
//!
//! Security invariants:
//! - Keys are NEVER written to `settings.json` or any plaintext file.
//! - Keys are NEVER logged (including `diagnostics.rs`).
//! - Keys are NEVER returned to the frontend; only boolean configured status is exposed.
//! - On non-Windows targets (used for cross-platform tests), an in-memory store is used.

use crate::translate_provider::TranslateProviderId;

#[cfg(target_os = "windows")]
const KEYRING_SERVICE: &str = "nexora";

fn account_name(provider: TranslateProviderId) -> Result<&'static str, String> {
    match provider {
        TranslateProviderId::GoogleCloud => Ok("translate_api_key_google_cloud"),
        TranslateProviderId::Gemini => Ok("translate_api_key_gemini"),
        TranslateProviderId::None
        | TranslateProviderId::Local
        | TranslateProviderId::GoogleFree => {
            Err("This translation provider does not use an API key.".to_string())
        }
    }
}

#[cfg(not(target_os = "windows"))]
static TEST_STORE: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<String, String>>,
> = std::sync::OnceLock::new();

#[cfg(not(target_os = "windows"))]
fn test_store() -> &'static std::sync::Mutex<std::collections::HashMap<String, String>> {
    TEST_STORE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Saves or updates the API key for `provider` in OS secure storage.
pub fn set_api_key(provider: TranslateProviderId, api_key: &str) -> Result<(), String> {
    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        return Err("API key cannot be empty.".to_string());
    }
    let account = account_name(provider)?;

    #[cfg(target_os = "windows")]
    {
        let entry = keyring::Entry::new(KEYRING_SERVICE, account)
            .map_err(|_| "Failed to access Windows Credential Manager.".to_string())?;
        entry
            .set_password(trimmed)
            .map_err(|_| "Failed to save API key to Windows Credential Manager.".to_string())?;
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    {
        let mut map = test_store().lock().unwrap_or_else(|e| e.into_inner());
        map.insert(account.to_string(), trimmed.to_string());
        Ok(())
    }
}

/// Reads the API key for `provider` internally (never expose to frontend or logs).
pub fn get_api_key(provider: TranslateProviderId) -> Option<String> {
    let account = account_name(provider).ok()?;

    #[cfg(target_os = "windows")]
    {
        let entry = keyring::Entry::new(KEYRING_SERVICE, account).ok()?;
        entry
            .get_password()
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    #[cfg(not(target_os = "windows"))]
    {
        let map = test_store().lock().unwrap_or_else(|e| e.into_inner());
        map.get(account).cloned().filter(|s| !s.is_empty())
    }
}

/// Deletes the stored API key for `provider`. Succeeds if already absent.
pub fn delete_api_key(provider: TranslateProviderId) -> Result<(), String> {
    let account = account_name(provider)?;

    #[cfg(target_os = "windows")]
    {
        let entry = keyring::Entry::new(KEYRING_SERVICE, account)
            .map_err(|_| "Failed to access Windows Credential Manager.".to_string())?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("Failed to delete API key from Windows Credential Manager.".to_string()),
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let mut map = test_store().lock().unwrap_or_else(|e| e.into_inner());
        map.remove(account);
        Ok(())
    }
}

/// Returns `true` if a non-empty API key is stored for `provider`.
pub fn has_api_key(provider: TranslateProviderId) -> bool {
    get_api_key(provider).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_cloud_providers() {
        assert!(set_api_key(TranslateProviderId::None, "secret").is_err());
        assert!(set_api_key(TranslateProviderId::Local, "secret").is_err());
        assert!(!has_api_key(TranslateProviderId::None));
    }

    #[test]
    fn rejects_blank_api_key() {
        assert!(set_api_key(TranslateProviderId::Gemini, "   ").is_err());
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn roundtrips_and_deletes_in_memory_store() {
        set_api_key(TranslateProviderId::GoogleCloud, "test-key-123").expect("save");
        assert!(has_api_key(TranslateProviderId::GoogleCloud));
        assert_eq!(
            get_api_key(TranslateProviderId::GoogleCloud).as_deref(),
            Some("test-key-123")
        );
        delete_api_key(TranslateProviderId::GoogleCloud).expect("delete");
        assert!(!has_api_key(TranslateProviderId::GoogleCloud));
    }
}
