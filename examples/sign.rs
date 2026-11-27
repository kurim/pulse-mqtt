//! Signs a release for the self-update: `sign <file> <trusted comment>` writes
//! `<file>.minisig`. Used by the release workflow, because rsign can only ask for the
//! password on a console. The key comes from MINISIGN_SECRET_KEY and MINISIGN_PASSWORD.
//! The result is checked against `update.pub`, the key built into the app.

use minisign_verify::{PublicKey, Signature};
use std::error::Error;

/// Returns the signature file content, after checking it with the public key.
fn sign(
    secret_key: &str,
    password: String,
    public_key: &str,
    data: &[u8],
    comment: &str,
) -> Result<String, Box<dyn Error>> {
    let secret_key =
        minisign::SecretKeyBox::from_string(secret_key)?.into_secret_key(Some(password))?;
    let signature = minisign::sign(None, &secret_key, data, Some(comment), None)?.into_string();
    // Catches a secret key that does not belong to the app.
    PublicKey::decode(public_key)?.verify(data, &Signature::decode(&signature)?, false)?;
    Ok(signature)
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = std::env::args().skip(1);
    let (Some(file), Some(comment)) = (arguments.next(), arguments.next()) else {
        return Err("usage: sign <file> <trusted comment>".into());
    };
    let signature = sign(
        &std::env::var("MINISIGN_SECRET_KEY")?,
        std::env::var("MINISIGN_PASSWORD").unwrap_or_default(),
        include_str!("../update.pub"),
        &std::fs::read(&file)?,
        &comment,
    )?;
    std::fs::write(format!("{file}.minisig"), signature)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_only_with_the_matching_key_and_password() {
        let new_pair = || {
            let keys = minisign::KeyPair::generate_encrypted_keypair(Some("test-only".into()));
            let keys = keys.unwrap();
            (
                keys.pk.to_box().unwrap().into_string(),
                keys.sk.to_box(None).unwrap().into_string(),
            )
        };
        let ((public, secret), (other_public, _)) = (new_pair(), new_pair());
        let run = |password: &str, public: &str| {
            sign(
                &secret,
                password.into(),
                public,
                b"program",
                "version:1.0.0",
            )
        };
        assert!(run("test-only", &public).is_ok());
        assert!(run("wrong", &public).is_err());
        assert!(run("test-only", &other_public).is_err());
    }
}
