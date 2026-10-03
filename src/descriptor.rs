//! Output descriptors: creation (from a master key), parsing and validation.
//!
//! A wallet is defined by two descriptors:
//! - **external** (`.../0/*`): receive addresses handed out to payers.
//! - **internal** (`.../1/*`): change addresses used by the wallet itself.

use bdk_wallet::KeychainKind;
use bdk_wallet::bitcoin::bip32::Xpriv;
use bdk_wallet::bitcoin::secp256k1::Secp256k1;
use bdk_wallet::bitcoin::{Network, NetworkKind};
use bdk_wallet::descriptor::{DescriptorError, ExtendedDescriptor, IntoWalletDescriptor};
use bdk_wallet::keys::KeyError;
use bdk_wallet::miniscript::descriptor::KeyMap;
use bdk_wallet::template::{Bip84, DescriptorTemplate};

use crate::error::{Result, WalletError};

/// External + internal descriptor strings for one wallet account.
///
/// When produced by [`from_master_key`] these contain **private keys**; treat
/// them like the mnemonic itself and never log them.
#[derive(Clone, PartialEq, Eq)]
pub struct DescriptorPair {
    pub external: String,
    pub internal: String,
}

// Hand-written so private descriptors never end up in logs via `{:?}`.
impl std::fmt::Debug for DescriptorPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DescriptorPair { .. }")
    }
}

/// Build BIP84 (native segwit, `wpkh`) descriptors for account 0:
/// `m/84'/<coin>'/0'/0/*` (external) and `m/84'/<coin>'/0'/1/*` (internal).
pub fn from_master_key(master: Xpriv, network: Network) -> Result<DescriptorPair> {
    let build = |keychain| -> Result<String> {
        let (descriptor, key_map, _) = Bip84(master, keychain)
            .build(NetworkKind::from(network))
            .map_err(map_descriptor_error)?;
        Ok(descriptor.to_string_with_secret(&key_map))
    };
    Ok(DescriptorPair {
        external: build(KeychainKind::External)?,
        internal: build(KeychainKind::Internal)?,
    })
}

/// Check that `descriptor` parses, matches `network`, and can derive many addresses.
pub fn validate(descriptor: &str, network: Network) -> Result<()> {
    parse(descriptor, network).map(|_| ())
}

/// Validate `descriptor` and split it into its public form and the private
/// keys it contains (empty for a public-key descriptor).
pub(crate) fn parse(descriptor: &str, network: Network) -> Result<(ExtendedDescriptor, KeyMap)> {
    let secp = Secp256k1::new();
    let (parsed, key_map) = descriptor
        .into_wallet_descriptor(&secp, NetworkKind::from(network))
        .map_err(map_descriptor_error)?;

    // Without a `*` the descriptor describes exactly one script, so the wallet
    // could never hand out a fresh address.
    if !parsed.has_wildcard() {
        return Err(WalletError::InvalidDescriptor(
            "descriptor has no wildcard (`*`), so it cannot derive new addresses".into(),
        ));
    }
    Ok((parsed, key_map))
}

pub(crate) fn map_descriptor_error(err: DescriptorError) -> WalletError {
    match err {
        DescriptorError::Key(KeyError::InvalidNetworkKind) => WalletError::NetworkMismatch(
            "descriptor keys are not valid for the requested network".into(),
        ),
        other => WalletError::InvalidDescriptor(other.to_string()),
    }
}
