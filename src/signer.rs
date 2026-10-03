//! Signing with keys owned by this crate.
//!
//! The inner BDK wallet only ever sees public descriptors; the private keys
//! from the wallet's descriptors live here instead. (BDK 3.2 deprecated
//! wallet-held signers in favour of caller-owned `SignersContainer`s.)

use bdk_wallet::bitcoin::Psbt;
use bdk_wallet::descriptor::ExtendedDescriptor;
use bdk_wallet::miniscript::descriptor::KeyMap;
use bdk_wallet::signer::SignersContainer;
use bdk_wallet::SignOptions;

use crate::error::{Result, WalletError};

/// Signers for the wallet's keychains. Empty for a watch-only wallet.
#[derive(Default)]
pub(crate) struct Signers(Vec<SignersContainer>);

impl Signers {
    /// Add signers for the private keys in `key_map`, which belong to `descriptor`.
    pub(crate) fn add(&mut self, key_map: KeyMap, descriptor: &ExtendedDescriptor) {
        if !key_map.is_empty() {
            let secp = bdk_wallet::bitcoin::secp256k1::Secp256k1::new();
            self.0.push(SignersContainer::build(key_map, descriptor, &secp));
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Sign every input we hold keys for, then try to finalize.
    ///
    /// Returns `true` if the PSBT is fully signed and finalized.
    pub(crate) fn sign(&self, wallet: &bdk_wallet::Wallet, psbt: &mut Psbt) -> Result<bool> {
        if self.is_empty() {
            return Err(WalletError::WatchOnly);
        }
        let containers: Vec<&SignersContainer> = self.0.iter().collect();
        wallet
            .sign_with_signers(psbt, &containers, SignOptions::default())
            .map_err(|e| WalletError::Signing(e.to_string()))
    }
}
