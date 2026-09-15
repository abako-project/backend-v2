use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use generated_contracts::{AccountId32, PrincipalId, Sr25519Signature, WalletId};
use std::{
    fmt::Write as _,
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};
use subxt_signer::sr25519::Keypair;
use zeroize::Zeroizing;

use crate::store::Error;

pub(crate) type Secret = Zeroizing<[u8; 32]>;

pub(crate) fn random<const N: usize>() -> Result<[u8; N], Error> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).map_err(|_| Error::Entropy)?;
    Ok(bytes)
}

pub(crate) fn account(seed: &[u8; 32]) -> Result<AccountId32, Error> {
    let key = Keypair::from_secret_key(*seed).map_err(|_| Error::KeyUnavailable)?;
    Ok(AccountId32::from_bytes(key.public_key().0))
}

pub(crate) struct EncryptedSeed {
    pub(crate) ciphertext: Vec<u8>,
    pub(crate) nonce: [u8; 24],
}

// Fixed-width identities and discriminants make the associated data unambiguous.
fn aad(wallet: WalletId, principal: Option<PrincipalId>) -> Vec<u8> {
    let mut data = b"kunveno:custody:sr25519:format=1:key=1:".to_vec();
    data.extend_from_slice(wallet.as_bytes());
    data.push(u8::from(principal.is_some()));
    if let Some(principal) = principal {
        data.extend_from_slice(principal.as_bytes());
    }
    data
}

pub(crate) fn encrypt(
    master: &[u8; 32],
    seed: &[u8; 32],
    wallet: WalletId,
    principal: Option<PrincipalId>,
) -> Result<EncryptedSeed, Error> {
    let nonce = random()?;
    let cipher = XChaCha20Poly1305::new_from_slice(master).map_err(|_| Error::KeyUnavailable)?;
    let ciphertext = cipher
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: seed,
                aad: &aad(wallet, principal),
            },
        )
        .map_err(|_| Error::KeyUnavailable)?;
    Ok(EncryptedSeed { ciphertext, nonce })
}

pub(crate) fn decrypt(
    master: &[u8; 32],
    encrypted: &EncryptedSeed,
    wallet: WalletId,
    principal: Option<PrincipalId>,
) -> Result<Secret, Error> {
    let cipher = XChaCha20Poly1305::new_from_slice(master).map_err(|_| Error::KeyUnavailable)?;
    let plaintext = Zeroizing::new(
        cipher
            .decrypt(
                &XNonce::from(encrypted.nonce),
                Payload {
                    msg: &encrypted.ciphertext,
                    aad: &aad(wallet, principal),
                },
            )
            .map_err(|_| Error::KeyUnavailable)?,
    );
    if plaintext.len() != 32 {
        return Err(Error::KeyUnavailable);
    }
    let mut seed = Zeroizing::new([0; 32]);
    seed.copy_from_slice(&plaintext);
    Ok(seed)
}

pub(crate) fn sign(seed: &[u8; 32], bytes: &[u8]) -> Result<Sr25519Signature, Error> {
    let key = Keypair::from_secret_key(*seed).map_err(|_| Error::KeyUnavailable)?;
    Ok(Sr25519Signature::from_bytes(key.sign(bytes).0))
}

pub(crate) fn read_secret(path: &str) -> Result<Secret, Error> {
    let text = read_private(Path::new(path))?;
    let text = text.trim().strip_prefix("0x").unwrap_or(text.trim());
    if text.len() != 64 || !text.is_ascii() {
        return Err(Error::Configuration);
    }
    let mut result = Zeroizing::new([0; 32]);
    for (index, byte) in result.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
            .map_err(|_| Error::Configuration)?;
    }
    Ok(result)
}

pub(crate) fn read_token(path: &str) -> Result<Zeroizing<String>, Error> {
    let value = read_private(Path::new(path))?;
    let value = Zeroizing::new(value.trim().to_owned());
    if value.len() < 32 || value.len() > 256 || !value.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(Error::Configuration);
    }
    Ok(value)
}

fn read_private(path: &Path) -> Result<Zeroizing<String>, Error> {
    let mut file = fs::File::open(path).map_err(|_| Error::Configuration)?;
    let metadata = file.metadata().map_err(|_| Error::Configuration)?;
    if !metadata.is_file() || metadata.len() > 1024 || metadata.permissions().mode() & 0o077 != 0 {
        return Err(Error::Configuration);
    }
    let mut text = Zeroizing::new(String::new());
    file.read_to_string(&mut text)
        .map_err(|_| Error::Configuration)?;
    Ok(text)
}

pub(crate) fn init_dev_secrets(directory: &Path) -> Result<(), Error> {
    if !directory.exists() {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(directory)
            .map_err(|_| Error::Configuration)?;
    }
    let metadata = fs::symlink_metadata(directory).map_err(|_| Error::Configuration)?;
    if !metadata.is_dir() || metadata.permissions().mode() & 0o077 != 0 {
        return Err(Error::Configuration);
    }
    let names = [
        "master-key.hex",
        "root-seed.hex",
        "root-account.hex",
        "service-token",
        "bootstrap-admin-password",
    ];
    // Refuse partial rotation: every destination must be absent before generating secrets.
    if names
        .iter()
        .any(|name| directory.join(name).symlink_metadata().is_ok())
    {
        return Err(Error::Configuration);
    }
    let master = Zeroizing::new(random::<32>()?);
    let seed = Zeroizing::new(random::<32>()?);
    let service = Zeroizing::new(random::<32>()?);
    let password = Zeroizing::new(random::<32>()?);
    let public = account(&seed)?;
    for (name, bytes) in
        names
            .into_iter()
            .zip([&*master, &*seed, public.as_bytes(), &*service, &*password])
    {
        let mut value = Zeroizing::new(String::with_capacity(65));
        for byte in bytes {
            write!(value, "{byte:02x}").map_err(|_| Error::Configuration)?;
        }
        value.push('\n');
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(directory.join(name))
            .map_err(|_| Error::Configuration)?;
        file.write_all(value.as_bytes())
            .map_err(|_| Error::Configuration)?;
        file.sync_all().map_err(|_| Error::Configuration)?;
    }
    fs::File::open(directory)
        .and_then(|file| file.sync_all())
        .map_err(|_| Error::Configuration)?;
    Ok(())
}
