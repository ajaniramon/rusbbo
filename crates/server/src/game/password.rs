//! Password hashes as the Java server and website write them (Java: `Havana.getPasswordEncoder`,
//! spring-security's `Argon2PasswordEncoder(16, 32, 1, 65536, 2)`): Argon2id, 16-byte salt,
//! 32-byte hash, 1 lane, 64 MiB, 2 iterations, in the PHC string format
//! (`$argon2id$v=19$m=65536,t=2,p=1$<salt>$<hash>`, standard Base64 without padding).
//!
//! Verifying takes 64 MiB for a moment. On a Raspberry Pi that is a lot to do many times at once,
//! so verifications run one at a time.

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use parking_lot::Mutex;

/// One Argon2 computation at a time (each needs 64 MiB).
static ARGON2_SLOT: Mutex<()> = Mutex::new(());

fn encoder() -> Argon2<'static> {
    let params = Params::new(65536, 2, 1, Some(32)).expect("valid Argon2 parameters");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

/// Java: `PlayerManager.createPassword`.
pub fn hash(password: &str) -> anyhow::Result<String> {
    use rand::RngExt;
    let salt_bytes: [u8; 16] = rand::rng().random();
    let salt = SaltString::encode_b64(&salt_bytes).map_err(|e| anyhow::anyhow!("salt: {e}"))?;
    let _slot = ARGON2_SLOT.lock();
    let hash = encoder()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("hashing: {e}"))?;
    Ok(hash.to_string())
}

/// Java: `PlayerManager.passwordMatches(databasePassword, enteredPassword)`: the parameters come
/// from the stored hash. Anything that is not a valid Argon2 hash (an empty password column)
/// never matches, as in spring-security. The password is hashed as UTF-8, like Java's.
pub fn matches(stored: &str, password: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(stored) else {
        return false;
    };
    if !parsed.algorithm.as_str().starts_with("argon2") {
        return false;
    }
    let _slot = ARGON2_SLOT.lock();
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// The Java hash of the seed user tracepass, for other modules' tests.
#[cfg(test)]
pub fn tests_java_hash() -> &'static str {
    tests::JAVA_HASH
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Written by Java's `new Argon2PasswordEncoder(16, 32, 1, 65536, 2).encode("tracepass123")`
    /// (oracle/seed.sql, user tracepass).
    pub const JAVA_HASH: &str = "$argon2id$v=19$m=65536,t=2,p=1$+dssL2Co/b/pOlmPSnwK3g$\
                             96Wt7oEYdSt0feXvqaUSVscIU2BuYuX5Ac9wCAfrHVc";

    #[test]
    fn verifies_java_hashes() {
        assert!(matches(JAVA_HASH, "tracepass123"));
        assert!(!matches(JAVA_HASH, "tracepass124"));
        assert!(!matches(JAVA_HASH, ""));
        assert!(!matches("", "tracepass123"));
        assert!(!matches("plain-text", "plain-text"));
    }

    #[test]
    fn hashes_like_java() {
        let h = hash("changeme123").unwrap();
        assert!(h.starts_with("$argon2id$v=19$m=65536,t=2,p=1$"), "{h}");
        // 16-byte salt and 32-byte hash in unpadded Base64.
        let parts: Vec<&str> = h.split('$').collect();
        assert_eq!(parts[4].len(), 22);
        assert_eq!(parts[5].len(), 43);
        assert!(matches(&h, "changeme123"));
        assert!(!matches(&h, "changeme124"));
    }
}
