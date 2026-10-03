//! The main [`Wallet`] type.

use bdk_wallet::bitcoin::{Address, FeeRate, Network, Psbt, Transaction, Txid};
use bdk_wallet::descriptor::ExtendedDescriptor;
use bdk_wallet::{Balance, KeychainKind};

use crate::address::{self, AddressInfo};
use crate::backend::ChainBackend;
use crate::descriptor;
use crate::error::{Result, WalletError};
use crate::keys;
use crate::signer::Signers;
use crate::state::{TxRecord, Utxo};
use crate::sync::{self, SyncSummary, unix_now};
use crate::tx::{self, Recipient};

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
    signers: Signers,
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
        let mut signers = Signers::default();
        let external = parse_descriptor(external, network, &mut signers)?;
        let internal = internal
            .map(|internal| parse_descriptor(internal, network, &mut signers))
            .transpose()?;
        let inner = create_inner(external, internal, network)?;
        Ok(Self {
            backend: NoBackend,
            inner,
            signers,
            birthday: 0,
        })
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

    /// Unspent outputs owned by the wallet, confirmed or not, including
    /// [reserved](Utxo::reserved) ones.
    pub fn list_utxos(&self) -> Vec<Utxo> {
        self.inner
            .list_unspent()
            .map(|output| {
                let reserved = self.inner.is_outpoint_locked(output.outpoint);
                Utxo { reserved, ..Utxo::from(output) }
            })
            .collect()
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
            signers: self.signers,
            birthday: self.birthday,
        }
    }

    /// Build an unsigned transaction paying `recipients`, with change back to
    /// a fresh internal address.
    ///
    /// Spends confirmed and unconfirmed UTXOs as of the last
    /// [`Wallet::sync`]. The selected UTXOs are **reserved** so the next build
    /// won't spend them too; the reservation ends when the transaction is
    /// [broadcast](Wallet::broadcast) or [cancelled](Wallet::cancel_tx).
    /// Reservations are in memory only.
    pub fn build_tx(&mut self, recipients: &[Recipient], fee_rate: FeeRate) -> Result<Psbt> {
        tx::build(&mut self.inner, recipients, fee_rate)
    }

    /// Build an unsigned transaction sending the whole balance to `address`,
    /// with no change output. The fee is taken from the amount sent.
    /// Reserved UTXOs are left out.
    pub fn build_drain_tx(&mut self, address: &Address, fee_rate: FeeRate) -> Result<Psbt> {
        tx::build_drain(&mut self.inner, address, fee_rate)
    }

    /// Build an unsigned replacement for the unconfirmed wallet transaction
    /// `txid` that pays `fee_rate` (replace-by-fee). Recipients are unchanged;
    /// the higher fee comes out of the change, adding inputs if needed.
    ///
    /// Sign and [broadcast](Wallet::broadcast) it like any other transaction;
    /// the original is then dropped from the wallet. Fails with
    /// [`WalletError::FeeBump`] if `txid` is unknown, confirmed or not
    /// replaceable, or [`WalletError::FeeRateTooLow`] if `fee_rate` doesn't
    /// beat the original by enough for nodes to accept the replacement.
    pub fn bump_fee(&mut self, txid: Txid, fee_rate: FeeRate) -> Result<Psbt> {
        tx::bump(&mut self.inner, txid, fee_rate)
    }

    /// Discard a built transaction that won't be broadcast: release its
    /// reserved UTXOs and let its change address be reused.
    pub fn cancel_tx(&mut self, psbt: &Psbt) {
        tx::cancel(&mut self.inner, &psbt.unsigned_tx);
    }

    /// Sign every input of `psbt` this wallet has keys for, then finalize it
    /// if possible.
    ///
    /// Returns `true` when the PSBT is fully signed and finalized, so
    /// [`Psbt::extract_tx`] will succeed. Returns [`WalletError::WatchOnly`]
    /// if the wallet holds no private keys.
    pub fn sign(&self, psbt: &mut Psbt) -> Result<bool> {
        self.signers.sign(&self.inner, psbt)
    }

    /// Whether the wallet was created from public-key descriptors only and so
    /// cannot [`sign`](Wallet::sign).
    pub fn is_watch_only(&self) -> bool {
        self.signers.is_empty()
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

    /// Fee rate expected to confirm a transaction within `target_blocks`
    /// blocks (1 = next block), for passing to [`build_tx`](Wallet::build_tx).
    ///
    /// Never returns less than the 1 sat/vB minimum nodes relay. Returns
    /// [`WalletError::FeeEstimation`] for a zero target or when the backend
    /// has no estimate; callers typically fall back to a fixed rate.
    pub fn estimate_fee(&self, target_blocks: u16) -> Result<FeeRate> {
        if target_blocks == 0 {
            return Err(WalletError::FeeEstimation(
                "confirmation target must be at least 1 block".into(),
            ));
        }
        let rate = self.backend.estimate_fee(target_blocks)?;
        Ok(rate.max(FeeRate::BROADCAST_MIN))
    }

    /// Submit a signed transaction through the backend.
    ///
    /// On success the transaction is also recorded as unconfirmed, so
    /// [`balance`](Wallet::balance) and [`list_utxos`](Wallet::list_utxos)
    /// reflect the spend (and any change) without waiting for the next sync.
    /// Any wallet transaction it replaces is dropped, and its inputs'
    /// reservations end. On failure the wallet is unchanged and the inputs
    /// stay reserved, so it can be retried or [cancelled](Wallet::cancel_tx).
    pub fn broadcast(&mut self, tx: &Transaction) -> Result<Txid> {
        let txid = self.backend.broadcast(tx)?;
        tx::record_broadcast(&mut self.inner, tx, unix_now());
        Ok(txid)
    }
}

// `Debug` shows only non-secret identity info; `signers` holds private keys.
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

/// Validate `descriptor`, move its private keys (if any) into `signers` and
/// return the public descriptor.
fn parse_descriptor(descriptor: &str, network: Network, signers: &mut Signers) -> Result<ExtendedDescriptor> {
    let (public, key_map) = descriptor::parse(descriptor, network)?;
    signers.add(key_map, &public);
    Ok(public)
}

fn create_inner(
    external: ExtendedDescriptor,
    internal: Option<ExtendedDescriptor>,
    network: Network,
) -> Result<bdk_wallet::Wallet> {
    let params = match internal {
        Some(internal) => bdk_wallet::Wallet::create(external, internal),
        None => bdk_wallet::Wallet::create_single(external),
    };
    params
        .network(network)
        .create_wallet_no_persist()
        .map_err(|e| WalletError::WalletCreation(e.to_string()))
}
