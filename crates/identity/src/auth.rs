use anyhow::Result;

use sha2::{Digest, Sha256};

const PREFIX_LEN: usize = 5;

pub fn hash_api_key(key: &str) -> Result<(String, String)> {
    let hash = Sha256::digest(key.as_bytes());
    let hash_str = String::from_utf8(hash.to_ascii_uppercase())?;

    Ok((key[0..PREFIX_LEN].to_owned(), hash_str))
}
