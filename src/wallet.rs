//! The main [`Wallet`] type.

use bdk_wallet::bitcoin::Network;
use bdk_wallet::{Balance, KeychainKind};

use crate::address::{self, AddressInfo};
use crate::backend::ChainBackend;
use crate::descriptor;
use crate::error::{Result, WalletError};
use crate::keys;
use crate::state::{TxRecord, Utxo};
use crate::sync::{self, SyncSummary};

/// Placeholder backend for a wallet that hasn't been connected to a chain.
///
/// Attach a real one with [`Wallet::with_backend`] to enable [`Wallet::sync`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoBackend;

/// A descriptor-based Bitcoin wallet.
///
/// `B` is the chain backend; [`Wallet::sync`] is available once it implements
/// [`ChainBackend`].
pub struct Wallet<B = NoBackend> {
    backend: B,
    inner: bdk_wallet::Wallet,
    birthday: u32,
}

impl Wallet<NoBackend> {
    /// Create a BIP84 (native segwit) wallet from a BIP39 mnemonic.
    ///
    /// Path: mnemonic → seed → master key → descriptors → wallet.
    pub fn from_mnemonic(phrase: &str, passphrase: Option<&str>, network: Network) -> Result<Self> {
        let descriptors = mnemonic_descriptors(phrase, passphrase, network)?;
        Self::from_descriptor(&descriptors.external, Some(&descriptors.internal), network)
    }

    /// Create a wallet from output descriptor strings.
    ///
    /// `internal` is the change descriptor. If omitted, change is sent to
    /// addresses from the external descriptor. Public-key (`xpub`/`tpub`)
    /// descriptors create a watch-only wallet.
    pub fn from_descriptor(external: &str, internal: Option<&str>, network: Network) -> Result<Self> {
        validate_descriptors(external, internal, network)?;
        let inner = create_inner(external, internal, network)?;
        Ok(Self { backend: NoBackend, inner, birthday: 0 })
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

    /// Confirmed and unconfirmed balance, as of the last [`Wallet::sync`].
    ///
    /// `confirmed` is spendable now; `trusted_pending` is unconfirmed change
    /// from our own transactions; `untrusted_pending` is unconfirmed incoming
    /// funds; `immature` is coinbase output not yet spendable.
    pub fn balance(&self) -> Balance {
        self.inner.balance()
    }

    /// Unspent outputs owned by the wallet, confirmed or not.
    pub fn list_utxos(&self) -> Vec<Utxo> {
        self.inner.list_unspent().map(Utxo::from).collect()
    }

    /// Wallet transactions, unconfirmed first, then newest to oldest.
    pub fn transactions(&self) -> Vec<TxRecord> {
        let mut records: Vec<TxRecord> = self
            .inner
            .transactions()
            .map(|wtx| {
                let tx = &wtx.tx_node.tx;
                let (sent, received) = self.inner.sent_and_received(tx);
                TxRecord {
                    txid: wtx.tx_node.txid,
                    sent,
                    received,
                    fee: self.inner.calculate_fee(tx).ok(),
                    confirmation: wtx.chain_position.into(),
                }
            })
            .collect();
        records.sort_by_key(|r| match r.confirmation {
            crate::Confirmation::Unconfirmed { .. } => (0, 0),
            crate::Confirmation::Confirmed { height, .. } => (1, u32::MAX - height),
        });
        records
    }

    /// Height of the last block the wallet has synced to (0 before the first sync).
    pub fn tip_height(&self) -> u32 {
        self.inner.latest_checkpoint().height()
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

    /// Set the wallet's birthday: the first block height that can contain its
    /// transactions. The first sync skips everything below it, which avoids
    /// scanning the whole chain for a new wallet.
    pub fn with_birthday(mut self, height: u32) -> Self {
        self.birthday = height;
        self
    }

    /// Borrow the attached backend.
    pub fn backend(&self) -> &B {
        &self.backend
    }

    /// Replace the backend, keeping all wallet state.
    pub fn with_backend<B2>(self, backend: B2) -> Wallet<B2> {
        Wallet {
            backend,
            inner: self.inner,
            birthday: self.birthday,
        }
    }
}

impl<B: ChainBackend> Wallet<B> {
    /// Update wallet state from the chain backend: new blocks (handling
    /// reorgs) and the current mempool.
    ///
    /// State is kept in memory only, so a new `Wallet` starts from scratch and
    /// rescans from its birthday on its first sync.
    pub fn sync(&mut self) -> Result<SyncSummary> {
        sync::sync(&mut self.inner, &self.backend, self.birthday)
    }
}

// `Debug` shows only non-secret identity info; the inner BDK wallet holds signers.
impl<B: std::fmt::Debug> std::fmt::Debug for Wallet<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Wallet")
            .field("network", &self.network())
            .field("tip_height", &self.tip_height())
            .field("backend", &self.backend)
            .finish_non_exhaustive()
    }
}

fn mnemonic_descriptors(
    phrase: &str,
    passphrase: Option<&str>,
    network: Network,
) -> Result<descriptor::DescriptorPair> {
    let mnemonic = keys::parse_mnemonic(phrase)?;
    let master = keys::master_key(&mnemonic, passphrase, network)?;
    descriptor::from_master_key(master, network)
}

fn validate_descriptors(external: &str, internal: Option<&str>, network: Network) -> Result<()> {
    descriptor::validate(external, network)?;
    if let Some(internal) = internal {
        descriptor::validate(internal, network)?;
    }
    Ok(())
}

fn create_inner(external: &str, internal: Option<&str>, network: Network) -> Result<bdk_wallet::Wallet> {
    let params = match internal {
        Some(internal) => bdk_wallet::Wallet::create(external.to_owned(), internal.to_owned()),
        None => bdk_wallet::Wallet::create_single(external.to_owned()),
    };
    params
        .network(network)
        .create_wallet_no_persist()
        .map_err(|e| WalletError::WalletCreation(e.to_string()))
}
