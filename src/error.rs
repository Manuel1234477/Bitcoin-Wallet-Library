//! Error type shared by every fallible operation in the library.

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
}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, WalletError>;
