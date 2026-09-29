//! Address derivation.
//!
//! `next_address` walks the flow:
//! next derivation index → descriptor/keychain → derive script → `Address`.
//! BDK tracks the "last revealed" index per keychain so an address is never
//! handed out twice.

use bdk_wallet::KeychainKind;
use bdk_wallet::bitcoin::Address;

/// An address revealed by the wallet, plus where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressInfo {
    /// Child index substituted for the descriptor's `*`.
    pub index: u32,
    /// The encoded address.
    pub address: Address,
    /// Which keychain (receive vs change) produced it.
    pub keychain: KeychainKind,
}

impl std::fmt::Display for AddressInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.address.fmt(f)
    }
}

impl From<bdk_wallet::AddressInfo> for AddressInfo {
    fn from(info: bdk_wallet::AddressInfo) -> Self {
        Self {
            index: info.index,
            address: info.address,
            keychain: info.keychain,
        }
    }
}

/// Reveal the next unused address on `keychain`, advancing its derivation index.
pub(crate) fn next_address(wallet: &mut bdk_wallet::Wallet, keychain: KeychainKind) -> AddressInfo {
    wallet.reveal_next_address(keychain).into()
}
