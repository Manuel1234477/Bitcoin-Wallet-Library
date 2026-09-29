//! Key material: mnemonic → seed → master extended private key.
//!
//! This is a thin layer over BDK's re-exports of `bip39` and `bitcoin::bip32`.
//! Child-key derivation (master → account → address keys) is not done here:
//! it is expressed as a derivation path inside a descriptor (see
//! [`crate::descriptor`]) and performed by BDK when addresses are revealed.

use bdk_wallet::bitcoin::Network;
use bdk_wallet::bitcoin::bip32::Xpriv;
use bdk_wallet::keys::bip39::Language;
use bdk_wallet::keys::{GeneratableKey, GeneratedKey};
use bdk_wallet::miniscript::Segwitv0;

use crate::error::{Result, WalletError};

pub use bdk_wallet::keys::bip39::{Mnemonic, WordCount};

/// Generate a fresh random English mnemonic using the OS random number generator.
pub fn generate_mnemonic(word_count: WordCount) -> Result<Mnemonic> {
    let generated: GeneratedKey<Mnemonic, Segwitv0> =
        Mnemonic::generate((word_count, Language::English)).map_err(|e| {
            WalletError::KeyDerivation(format!(
                "mnemonic generation failed: {}",
                e.map(|e| e.to_string()).unwrap_or_default()
            ))
        })?;
    Ok(generated.into_key())
}

/// Parse and checksum-validate an English mnemonic phrase.
pub fn parse_mnemonic(phrase: &str) -> Result<Mnemonic> {
    Mnemonic::parse_in(Language::English, phrase)
        .map_err(|e| WalletError::InvalidMnemonic(e.to_string()))
}

/// Derive the BIP32 master private key for `mnemonic` (+ optional BIP39 passphrase).
///
/// `network` only affects how the key serializes (`xprv` vs `tprv`); the
/// underlying key bytes are the same on every network.
pub fn master_key(mnemonic: &Mnemonic, passphrase: Option<&str>, network: Network) -> Result<Xpriv> {
    let seed = mnemonic.to_seed(passphrase.unwrap_or(""));
    Xpriv::new_master(network, &seed).map_err(|e| WalletError::KeyDerivation(e.to_string()))
}
