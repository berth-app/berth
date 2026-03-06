use anyhow::{Context, Result};
use security_framework::passwords::{
    delete_generic_password, get_generic_password, set_generic_password,
};

const SERVICE_NAME: &str = "com.runway.app";

/// Store a credential in the macOS Keychain.
/// The `key` identifies the credential (e.g. "target:my-vps:ssh-key", "aws:access-key").
/// The `value` is the secret data.
pub fn store_credential(key: &str, value: &str) -> Result<()> {
    set_generic_password(SERVICE_NAME, key, value.as_bytes())
        .context(format!("Failed to store credential for key '{key}'"))
}

/// Retrieve a credential from the macOS Keychain.
/// Returns None if not found.
pub fn get_credential(key: &str) -> Result<Option<String>> {
    match get_generic_password(SERVICE_NAME, key) {
        Ok(bytes) => {
            let s = String::from_utf8(bytes)
                .context(format!("Credential for key '{key}' contains invalid UTF-8"))?;
            Ok(Some(s))
        }
        Err(_) => Ok(None),
    }
}

/// Delete a credential from the macOS Keychain.
pub fn delete_credential(key: &str) -> Result<()> {
    match delete_generic_password(SERVICE_NAME, key) {
        Ok(()) => Ok(()),
        Err(_) => Ok(()), // Treat "not found" as success for idempotent deletes
    }
}

// ---------------------------------------------------------------------------
// Higher-level helpers
// ---------------------------------------------------------------------------

/// Store SSH key for a remote target.
pub fn store_ssh_key(target_name: &str, key_data: &str) -> Result<()> {
    store_credential(&format!("target:{target_name}:ssh-key"), key_data)
}

/// Get SSH key for a remote target.
pub fn get_ssh_key(target_name: &str) -> Result<Option<String>> {
    get_credential(&format!("target:{target_name}:ssh-key"))
}

/// Store AWS credentials (access key and secret key) for a named profile.
pub fn store_aws_credentials(profile: &str, access_key: &str, secret_key: &str) -> Result<()> {
    store_credential(&format!("aws:{profile}:access-key"), access_key)?;
    store_credential(&format!("aws:{profile}:secret-key"), secret_key)
}

/// Get AWS credentials for a named profile.
/// Returns `Some((access_key, secret_key))` if both are present, `None` otherwise.
pub fn get_aws_credentials(profile: &str) -> Result<Option<(String, String)>> {
    let access = get_credential(&format!("aws:{profile}:access-key"))?;
    let secret = get_credential(&format!("aws:{profile}:secret-key"))?;
    match (access, secret) {
        (Some(a), Some(s)) => Ok(Some((a, s))),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_get_delete_credential() {
        let key = "runway-test:unit-test-cred";
        let value = "s3cret-test-value-42";

        // Store
        store_credential(key, value).expect("store_credential failed");

        // Get
        let retrieved = get_credential(key).expect("get_credential failed");
        assert_eq!(retrieved, Some(value.to_string()));

        // Delete
        delete_credential(key).expect("delete_credential failed");

        // Confirm deleted
        let after_delete = get_credential(key).expect("get_credential after delete failed");
        assert_eq!(after_delete, None);
    }

    #[test]
    fn test_get_nonexistent_credential() {
        let result = get_credential("runway-test:does-not-exist-xyz-999")
            .expect("get_credential failed");
        assert_eq!(result, None);
    }

    #[test]
    fn test_delete_nonexistent_is_ok() {
        let result = delete_credential("runway-test:never-existed-abc-777");
        assert!(result.is_ok(), "deleting nonexistent credential should succeed");
    }

    #[test]
    fn test_ssh_key_helpers() {
        let target = "test-unit-target";
        let key_data = "-----BEGIN OPENSSH PRIVATE KEY-----\nfake\n-----END OPENSSH PRIVATE KEY-----";

        store_ssh_key(target, key_data).expect("store_ssh_key failed");
        let retrieved = get_ssh_key(target).expect("get_ssh_key failed");
        assert_eq!(retrieved, Some(key_data.to_string()));

        // Cleanup
        delete_credential(&format!("target:{target}:ssh-key")).unwrap();
    }

    #[test]
    fn test_aws_credentials_helpers() {
        let profile = "test-unit-profile";
        store_aws_credentials(profile, "AKIATEST123", "secret456")
            .expect("store_aws_credentials failed");

        let creds = get_aws_credentials(profile).expect("get_aws_credentials failed");
        assert_eq!(creds, Some(("AKIATEST123".to_string(), "secret456".to_string())));

        // Cleanup
        delete_credential(&format!("aws:{profile}:access-key")).unwrap();
        delete_credential(&format!("aws:{profile}:secret-key")).unwrap();
    }
}
