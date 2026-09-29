//! The main [`Wallet`] type.

use bdk_wallet::KeychainKind;
use bdk_wallet::bitcoin::Network;

use crate::address::{self, AddressInfo};
use crate::descriptor;
use crate::error::{Result, WalletError};
use crate::keys;

/// Placeholder backend used until blockchain synchronization exists.
///
/// Fixing the `Wallet<B>` shape now means a later phase can plug in a real
/// backend via [`Wallet::with_backend`] without changing existing call sites.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoBackend;

/// A descriptor-based Bitcoin wallet.
pub struct Wallet<B = NoBackend> {
    backend: B,
    inner: bdk_wallet::Wallet,
}

impl Wallet<NoBackend> {
    /// Create a BIP84 (native segwit) wallet from a BIP39 mnemonic.
    ///
    /// Path: mnemonic → seed → master key → descriptors → wallet.
    pub fn from_mnemonic(phrase: &str, passphrase: Option<&str>, network: Network) -> Result<Self> {
        let mnemonic = keys::parse_mnemonic(phrase)?;
        let master = keys::master_key(&mnemonic, passphrase, network)?;
        let descriptors = descriptor::from_master_key(master, network)?;
        Self::from_descriptor(&descriptors.external, Some(&descriptors.internal), network)
    }

    /// Create a wallet from output descriptor strings.
    ///
    /// `internal` is the change descriptor. If omitted, change is sent to
    /// addresses from the external descriptor. Public-key (`xpub`/`tpub`)
    /// descriptors create a watch-only wallet.
    pub fn from_descriptor(external: &str, internal: Option<&str>, network: Network) -> Result<Self> {
        descriptor::validate(external, network)?;
        if let Some(internal) = internal {
            descriptor::validate(internal, network)?;
        }

        let params = match internal {
            Some(internal) => bdk_wallet::Wallet::create(external.to_owned(), internal.to_owned()),
            None => bdk_wallet::Wallet::create_single(external.to_owned()),
        };
        let inner = params
            .network(network)
            .create_wallet_no_persist()
            .map_err(|e| WalletError::WalletCreation(e.to_string()))?;

        Ok(Self { backend: NoBackend, inner })
    }
}

impl<B> Wallet<B> {
    /// Reveal the next unused receive address.
    pub fn new_address(&mut self) -> Result<AddressInfo> {
        Ok(address::next_address(&mut self.inner, KeychainKind::External))
    }

    /// Reveal the next unused change address.
    pub fn new_change_address(&mut self) -> Result<AddressInfo> {
        Ok(address::next_address(&mut self.inner, KeychainKind::Internal))
    }

    /// The network this wallet operates on.
    pub fn network(&self) -> Network {
        self.inner.network()
    }

    /// Index that the next call to `new_address` (external) or
    /// `new_change_address` (internal) will use.
    pub fn next_derivation_index(&self, keychain: KeychainKind) -> u32 {
        self.inner.next_derivation_index(keychain)
    }

    /// Public (private-key-free) descriptor for `keychain`, safe to share or
    /// use to build a watch-only wallet.
    pub fn public_descriptor(&self, keychain: KeychainKind) -> String {
        self.inner.public_descriptor(keychain).to_string()
    }

    /// Borrow the attached backend.
    pub fn backend(&self) -> &B {
        &self.backend
    }

    /// Replace the backend, keeping all wallet state.
    pub fn with_backend<B2>(self, backend: B2) -> Wallet<B2> {
        Wallet { backend, inner: self.inner }
    }
}

// `Debug` shows only non-secret identity info; the inner BDK wallet holds signers.
impl<B: std::fmt::Debug> std::fmt::Debug for Wallet<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Wallet")
            .field("network", &self.network())
            .field("backend", &self.backend)
            .finish_non_exhaustive()
    }
}
