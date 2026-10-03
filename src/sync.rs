//! Chain synchronization against any [`ChainBackend`].
//!
//! 1. **Find the fork point**: walk the wallet's checkpoints from its tip down
//!    until one matches the backend's block at that height. Normally this is
//!    the wallet tip itself (one RPC call); after a reorg it is the last
//!    common block.
//! 2. **Apply blocks** above the fork point up to the backend tip. BDK keeps
//!    only wallet-relevant transactions, and a block that conflicts with a
//!    stored one invalidates the stale branch.
//! 3. **Apply the mempool**: add relevant unconfirmed transactions and mark
//!    wallet transactions that disappeared from the mempool as evicted.

use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use bdk_wallet::bitcoin::Txid;
use bdk_wallet::chain::BlockId;

use crate::backend::ChainBackend;
use crate::error::{Result, WalletError};

/// What a call to [`Wallet::sync`](crate::Wallet::sync) did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncSummary {
    /// Height of the backend tip the wallet is now synced to.
    pub tip_height: u32,
    /// Number of blocks fetched and applied.
    pub blocks_applied: u32,
    /// Height of the last block shared with the previous sync. Lower than the
    /// previous tip only if a reorg happened.
    pub fork_height: u32,
}

pub(crate) fn sync<B: ChainBackend>(
    wallet: &mut bdk_wallet::Wallet,
    backend: &B,
    scan_from_height: u32,
) -> Result<SyncSummary> {
    let tip = backend.tip()?;
    let fork = find_fork_point(wallet, backend, tip.height)?;

    let mut connected_to = fork;
    let mut blocks_applied = 0;
    for height in (fork.height + 1).max(scan_from_height)..=tip.height {
        let hash = backend.block_hash(height)?;
        let block = backend.block(&hash)?;
        wallet
            .apply_block_connected_to(&block, height, connected_to)
            .map_err(|e| WalletError::Sync(format!("block {height} does not connect: {e}")))?;
        connected_to = BlockId { height, hash };
        blocks_applied += 1;
    }

    apply_mempool(wallet, backend)?;

    Ok(SyncSummary {
        tip_height: tip.height,
        blocks_applied,
        fork_height: fork.height,
    })
}

/// Highest wallet checkpoint that is also in the backend's best chain.
fn find_fork_point<B: ChainBackend>(
    wallet: &bdk_wallet::Wallet,
    backend: &B,
    backend_tip_height: u32,
) -> Result<BlockId> {
    for cp in wallet.latest_checkpoint().iter() {
        if cp.height() > backend_tip_height {
            continue;
        }
        if backend.block_hash(cp.height())? == cp.hash() {
            return Ok(cp.block_id());
        }
    }
    // Even the genesis block differs: the backend is on another network.
    Err(WalletError::Sync(
        "backend chain shares no blocks with the wallet (wrong network?)".into(),
    ))
}

fn apply_mempool<B: ChainBackend>(wallet: &mut bdk_wallet::Wallet, backend: &B) -> Result<()> {
    let now = unix_now();
    let mempool = backend.mempool()?;
    let in_mempool: HashSet<Txid> = mempool.iter().map(|tx| tx.compute_txid()).collect();

    // Wallet txs that are still unconfirmed (after applying blocks) but no
    // longer in the mempool were replaced or dropped.
    let evicted: Vec<(Txid, u64)> = wallet
        .transactions()
        .filter(|tx| !tx.chain_position.is_confirmed())
        .map(|tx| tx.tx_node.txid)
        .filter(|txid| !in_mempool.contains(txid))
        .map(|txid| (txid, now))
        .collect();

    wallet.apply_unconfirmed_txs(mempool.into_iter().map(|tx| (tx, now)));
    wallet.apply_evicted_txs(evicted);
    Ok(())
}

pub(crate) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}
