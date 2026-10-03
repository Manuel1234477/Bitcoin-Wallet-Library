//! Read-only views of wallet state: UTXOs and transaction history.

use bdk_wallet::KeychainKind;
use bdk_wallet::bitcoin::{Amount, OutPoint, Txid};
use bdk_wallet::chain::{ChainPosition, ConfirmationBlockTime};

/// Where a transaction (or output) sits relative to the best chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirmation {
    /// Included in the block at `height`, mined at unix time `time`.
    Confirmed { height: u32, time: u64 },
    /// Not yet in a block. `last_seen` is the unix time it was last seen in
    /// the mempool, if ever.
    Unconfirmed { last_seen: Option<u64> },
}

impl Confirmation {
    pub fn is_confirmed(&self) -> bool {
        matches!(self, Self::Confirmed { .. })
    }
}

impl From<ChainPosition<ConfirmationBlockTime>> for Confirmation {
    fn from(position: ChainPosition<ConfirmationBlockTime>) -> Self {
        match position {
            ChainPosition::Confirmed { anchor, .. } => Self::Confirmed {
                height: anchor.block_id.height,
                time: anchor.confirmation_time,
            },
            ChainPosition::Unconfirmed { last_seen, .. } => Self::Unconfirmed { last_seen },
        }
    }
}

/// An unspent output owned by the wallet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Utxo {
    pub outpoint: OutPoint,
    pub value: Amount,
    /// Receive or change keychain that owns the output's script.
    pub keychain: KeychainKind,
    /// Derivation index of the owning address on `keychain`.
    pub derivation_index: u32,
    pub confirmation: Confirmation,
}

impl From<bdk_wallet::LocalOutput> for Utxo {
    fn from(output: bdk_wallet::LocalOutput) -> Self {
        Self {
            outpoint: output.outpoint,
            value: output.txout.value,
            keychain: output.keychain,
            derivation_index: output.derivation_index,
            confirmation: output.chain_position.into(),
        }
    }
}

/// A transaction that pays to or spends from the wallet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxRecord {
    pub txid: Txid,
    /// Total value of wallet-owned inputs.
    pub sent: Amount,
    /// Total value of outputs paying to the wallet (including change).
    pub received: Amount,
    /// `None` when some inputs are not the wallet's, so their values are unknown.
    pub fee: Option<Amount>,
    pub confirmation: Confirmation,
}
