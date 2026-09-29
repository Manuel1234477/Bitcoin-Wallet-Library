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

mod address;
pub mod descriptor;
mod error;
pub mod keys;
mod wallet;

pub use address::AddressInfo;
pub use error::{Result, WalletError};
pub use wallet::{NoBackend, Wallet};

// Re-exported so callers don't need a direct BDK/bitcoin dependency.
pub use bdk_wallet::KeychainKind;
pub use bdk_wallet::bitcoin::{Address, Network};
