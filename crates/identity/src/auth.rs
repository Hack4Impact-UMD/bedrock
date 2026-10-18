use rand::{RngExt, distr::Alphanumeric};
use sha2::{Digest, Sha256};

const KEY_TAG: &str = "bdrk_";
const RANDOM_LEN: usize = 40;
const PREFIX_LEN: usize = KEY_TAG.len() + 8;

/// Generates an API key string
pub fn generate_api_key() -> String {
    let random: String = rand::rng()
        .sample_iter(Alphanumeric)
        .take(RANDOM_LEN)
        .map(char::from)
        .collect();

    format!("{KEY_TAG}{random}")
}

/// Splits a key into its lookup prefix and SHA-256 hash. Returns `None` if the key is too short to have a prefix.
pub fn hash_api_key(key: &str) -> Option<(&str, Vec<u8>)> {
    let prefix = key.get(..PREFIX_LEN)?;
    let hash = Sha256::digest(key.as_bytes()).to_vec();

    Some((prefix, hash))
}
