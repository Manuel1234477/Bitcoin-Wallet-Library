//! Error type shared by every fallible operation in the library.

use bdk_wallet::bitcoin::{Amount, FeeRate};
use thiserror::Error;

/// Errors returned by the wallet library.
///
/// Variants carry a human-readable message rather than the underlying
/// BDK/bitcoin error types, so upgrading those dependencies never changes
/// this crate's public API.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum WalletError {
    /// The mnemonic phrase is malformed (unknown word, bad checksum, wrong length).
    #[error("invalid mnemonic: {0}")]
    InvalidMnemonic(String),

    /// The descriptor could not be parsed or is unusable by this wallet.
    #[error("invalid descriptor: {0}")]
    InvalidDescriptor(String),

    /// A key in the descriptor belongs to a different network (e.g. a `tpub` on mainnet).
    #[error("network mismatch: {0}")]
    NetworkMismatch(String),

    /// Deriving keys from the seed failed.
    #[error("key derivation failed: {0}")]
    KeyDerivation(String),

    /// BDK refused to build the wallet from otherwise valid descriptors.
    #[error("wallet creation failed: {0}")]
    WalletCreation(String),

    /// The chain backend failed (connection, RPC error, unexpected data).
    #[error("backend error: {0}")]
    Backend(String),

    /// The backend's chain does not connect to the wallet's chain (e.g. wrong network).
    #[error("sync failed: {0}")]
    Sync(String),

    /// The wallet's spendable outputs can't cover the amounts plus fee.
    #[error("insufficient funds: need {needed}, have {available}")]
    InsufficientFunds { needed: Amount, available: Amount },

    /// A payment destination is unusable: no recipients, wrong network, or an
    /// amount below the dust limit.
    #[error("invalid recipient: {0}")]
    InvalidRecipient(String),

    /// Building the transaction failed for another reason.
    #[error("transaction building failed: {0}")]
    TxBuild(String),

    /// A signer failed while signing the PSBT.
    #[error("signing failed: {0}")]
    Signing(String),

    /// The transaction can't be fee-bumped: unknown, already confirmed, or
    /// not replaceable.
    #[error("fee bump failed: {0}")]
    FeeBump(String),

    /// A replacement transaction must pay at least `required` (BIP125).
    #[error("fee rate too low: replacement needs at least {} sat/vB", required.to_sat_per_vb_ceil())]
    FeeRateTooLow { required: FeeRate },

    /// No fee estimate is available, or the confirmation target is invalid.
    #[error("fee estimation failed: {0}")]
    FeeEstimation(String),

    /// The wallet holds no private keys, so it cannot sign.
    #[error("wallet is watch-only and cannot sign")]
    WatchOnly,
}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, WalletError>;
