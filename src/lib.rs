//! A descriptor-based Bitcoin wallet library built on BDK.
//!
//! ```
//! use wallet_library::{Network, Wallet};
//!
//! let phrase = "abandon abandon abandon abandon abandon abandon \
//!               abandon abandon abandon abandon abandon about";
//! let mut wallet = Wallet::from_mnemonic(phrase, None, Network::Bitcoin)?;
//! let address = wallet.new_address()?;
//! assert_eq!(address.to_string(), "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu");
//! # Ok::<(), wallet_library::WalletError>(())
//! ```
//!
//! Syncing with a chain backend:
//!
//! ```no_run
//! use wallet_library::{Auth, BitcoindRpc, Network, Wallet};
//!
//! # let phrase = "abandon abandon abandon abandon abandon abandon \
//! #               abandon abandon abandon abandon abandon about";
//! let backend = BitcoindRpc::new("http://127.0.0.1:18443", Auth::CookieFile("/path/.cookie".into()))?;
//! let mut wallet = Wallet::from_mnemonic(phrase, None, Network::Regtest)?
//!     .with_backend(backend);
//! wallet.sync()?;
//! println!("confirmed: {}", wallet.balance().confirmed);
//! for utxo in wallet.list_utxos() {
//!     println!("{} {}", utxo.outpoint, utxo.value);
//! }
//! # Ok::<(), wallet_library::WalletError>(())
//! ```

mod address;
mod backend;
pub mod descriptor;
mod error;
pub mod keys;
mod state;
mod sync;
mod wallet;

pub use address::AddressInfo;
pub use backend::{Auth, BitcoindRpc, ChainBackend};
pub use error::{Result, WalletError};
pub use state::{Confirmation, TxRecord, Utxo};
pub use sync::SyncSummary;
pub use wallet::{NoBackend, Wallet};

// Re-exported so callers don't need a direct BDK/bitcoin dependency.
pub use bdk_wallet::chain::BlockId;
pub use bdk_wallet::bitcoin::{
    Address, Amount, Block, BlockHash, Network, OutPoint, Transaction, Txid,
};
pub use bdk_wallet::{Balance, KeychainKind};
