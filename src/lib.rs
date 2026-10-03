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
//!
//! Sending a payment: build an unsigned PSBT, sign it, broadcast it.
//!
//! ```no_run
//! # use wallet_library::{Auth, BitcoindRpc, Network, Wallet};
//! use std::str::FromStr;
//! use wallet_library::{Address, Amount, FeeRate, Recipient};
//!
//! # let phrase = "abandon abandon abandon abandon abandon abandon \
//! #               abandon abandon abandon abandon abandon about";
//! # let backend = BitcoindRpc::new("http://127.0.0.1:18443", Auth::None)?;
//! # let mut wallet = Wallet::from_mnemonic(phrase, None, Network::Regtest)?.with_backend(backend);
//! wallet.sync()?;
//! let to = Address::from_str("bcrt1qw508d6qejxtdg4y5r3zarvary0c5xw7kygt080")
//!     .unwrap()
//!     .require_network(Network::Regtest)
//!     .unwrap();
//! let fee_rate = FeeRate::from_sat_per_vb(2).unwrap();
//! let mut psbt = wallet.build_tx(&[Recipient::new(to, Amount::from_sat(50_000))], fee_rate)?;
//! assert!(wallet.sign(&mut psbt)?, "all inputs are ours, so the PSBT is finalized");
//! let tx = psbt.extract_tx().expect("finalized PSBT");
//! let txid = wallet.broadcast(&tx)?;
//! println!("sent {txid}");
//! # Ok::<(), wallet_library::WalletError>(())
//! ```

mod address;
mod backend;
pub mod descriptor;
mod error;
pub mod keys;
mod signer;
mod state;
mod sync;
mod tx;
mod wallet;

pub use address::AddressInfo;
pub use backend::{Auth, BitcoindRpc, ChainBackend};
pub use error::{Result, WalletError};
pub use state::{Confirmation, TxRecord, Utxo};
pub use sync::SyncSummary;
pub use tx::Recipient;
pub use wallet::{NoBackend, Wallet};

// Re-exported so callers don't need a direct BDK/bitcoin dependency.
pub use bdk_wallet::chain::BlockId;
pub use bdk_wallet::bitcoin::{
    Address, Amount, Block, BlockHash, FeeRate, Network, OutPoint, Psbt, Transaction, Txid,
};
pub use bdk_wallet::{Balance, KeychainKind};
