//! Pluggable chain backends.
//!
//! [`ChainBackend`] is the only thing [`Wallet::sync`](crate::Wallet::sync)
//! needs from the outside world: block hashes by height, full blocks, and the
//! mempool. Reorg detection and applying data to the wallet happen in the
//! wallet itself, so a backend is a thin, stateless data source and is easy to
//! mock in tests.

use bdk_wallet::bitcoin::{Block, BlockHash, Transaction, Txid};
use bdk_wallet::chain::BlockId;
use bitcoincore_rpc::{Client, RpcApi};

use crate::error::{Result, WalletError};

pub use bitcoincore_rpc::Auth;

/// A source of blockchain data for [`Wallet::sync`](crate::Wallet::sync).
///
/// Implementations report failures as [`WalletError::Backend`].
pub trait ChainBackend {
    /// Height and hash of the backend's best block.
    fn tip(&self) -> Result<BlockId>;

    /// Hash of the best-chain block at `height`.
    fn block_hash(&self, height: u32) -> Result<BlockHash>;

    /// The full block with hash `hash`.
    fn block(&self, hash: &BlockHash) -> Result<Block>;

    /// All transactions currently in the mempool.
    fn mempool(&self) -> Result<Vec<Transaction>>;

    /// Submit a signed transaction to the network.
    fn broadcast(&self, tx: &Transaction) -> Result<Txid>;
}

/// [`ChainBackend`] backed by a Bitcoin Core node's JSON-RPC interface.
pub struct BitcoindRpc {
    url: String,
    client: Client,
}

impl BitcoindRpc {
    /// Connect to the node at `url` (e.g. `http://127.0.0.1:18443`).
    ///
    /// No request is made here; connection problems surface on first use.
    pub fn new(url: &str, auth: Auth) -> Result<Self> {
        let client = Client::new(url, auth).map_err(backend_error)?;
        Ok(Self { url: url.to_owned(), client })
    }

    /// The underlying RPC client, for calls this crate doesn't wrap.
    pub fn client(&self) -> &Client {
        &self.client
    }
}

impl ChainBackend for BitcoindRpc {
    fn tip(&self) -> Result<BlockId> {
        let info = self.client.get_blockchain_info().map_err(backend_error)?;
        Ok(BlockId {
            height: info.blocks as u32,
            hash: info.best_block_hash,
        })
    }

    fn block_hash(&self, height: u32) -> Result<BlockHash> {
        self.client.get_block_hash(height.into()).map_err(backend_error)
    }

    fn block(&self, hash: &BlockHash) -> Result<Block> {
        self.client.get_block(hash).map_err(backend_error)
    }

    fn mempool(&self) -> Result<Vec<Transaction>> {
        let txids = self.client.get_raw_mempool().map_err(backend_error)?;
        let mut txs = Vec::with_capacity(txids.len());
        for txid in txids {
            match self.client.get_raw_transaction(&txid, None) {
                Ok(tx) => txs.push(tx),
                // The tx left the mempool (mined or evicted) between the two calls.
                Err(e) if is_not_found(&e) => {}
                Err(e) => return Err(backend_error(e)),
            }
        }
        Ok(txs)
    }

    fn broadcast(&self, tx: &Transaction) -> Result<Txid> {
        self.client.send_raw_transaction(tx).map_err(backend_error)
    }
}

// Hand-written so RPC credentials never end up in logs.
impl std::fmt::Debug for BitcoindRpc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BitcoindRpc").field("url", &self.url).finish_non_exhaustive()
    }
}

fn backend_error(err: bitcoincore_rpc::Error) -> WalletError {
    WalletError::Backend(err.to_string())
}

/// RPC_INVALID_ADDRESS_OR_KEY: "No such mempool or blockchain transaction".
fn is_not_found(err: &bitcoincore_rpc::Error) -> bool {
    matches!(
        err,
        bitcoincore_rpc::Error::JsonRpc(bitcoincore_rpc::jsonrpc::Error::Rpc(e)) if e.code == -5
    )
}
