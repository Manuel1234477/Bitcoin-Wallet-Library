//! Shared test helpers: an in-memory chain backend and fake payments.

#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;

use bdk_wallet::bitcoin::block::{Header, Version as BlockVersion};
use bdk_wallet::bitcoin::constants::genesis_block;
use bdk_wallet::bitcoin::hashes::Hash;
use bdk_wallet::bitcoin::transaction::Version as TxVersion;
use bdk_wallet::bitcoin::{
    CompactTarget, ScriptBuf, Sequence, TxIn, TxMerkleNode, TxOut, Witness, absolute,
};
use wallet_library::{
    Address, Amount, Block, BlockHash, BlockId, ChainBackend, FeeRate, Network, OutPoint,
    Transaction, Txid, WalletError,
};

pub const MNEMONIC: &str = "abandon abandon abandon abandon abandon abandon \
                        abandon abandon abandon abandon abandon about";

/// A fake regtest chain whose blocks and mempool tests can edit between syncs.
#[derive(Clone)]
pub struct MockChain(Rc<RefCell<State>>);

struct State {
    blocks: Vec<Block>,
    mempool: Vec<Transaction>,
    nonce: u32,
    fail: bool,
    /// Rate returned by `estimate_fee` for any target; `None` = no estimate.
    fee_rate: Option<FeeRate>,
}

impl MockChain {
    pub fn new() -> Self {
        Self(Rc::new(RefCell::new(State {
            blocks: vec![genesis_block(Network::Regtest)],
            mempool: Vec::new(),
            nonce: 0,
            fail: false,
            fee_rate: None,
        })))
    }

    /// Append a block containing `txs`, removing them from the mempool.
    pub fn mine(&self, txs: Vec<Transaction>) {
        let mut s = self.0.borrow_mut();
        s.nonce += 1;
        let prev = s.blocks.last().unwrap();
        let header = Header {
            version: BlockVersion::TWO,
            prev_blockhash: prev.block_hash(),
            merkle_root: TxMerkleNode::all_zeros(),
            time: prev.header.time + 600,
            bits: CompactTarget::from_consensus(0x207f_ffff),
            nonce: s.nonce,
        };
        let mined: Vec<Txid> = txs.iter().map(|tx| tx.compute_txid()).collect();
        s.mempool.retain(|tx| !mined.contains(&tx.compute_txid()));
        s.blocks.push(Block { header, txdata: txs });
    }

    pub fn mine_empty(&self, n: usize) {
        for _ in 0..n {
            self.mine(vec![]);
        }
    }

    /// Drop every block at `height` and above (a reorg once new blocks are mined).
    pub fn disconnect_from(&self, height: usize) {
        self.0.borrow_mut().blocks.truncate(height);
    }

    pub fn add_to_mempool(&self, tx: Transaction) {
        self.0.borrow_mut().mempool.push(tx);
    }

    pub fn clear_mempool(&self) {
        self.0.borrow_mut().mempool.clear();
    }

    pub fn set_fee_rate(&self, fee_rate: Option<FeeRate>) {
        self.0.borrow_mut().fee_rate = fee_rate;
    }

    pub fn set_failing(&self, fail: bool) {
        self.0.borrow_mut().fail = fail;
    }

    fn check(&self) -> wallet_library::Result<()> {
        if self.0.borrow().fail {
            return Err(WalletError::Backend("connection refused".into()));
        }
        Ok(())
    }
}

impl ChainBackend for MockChain {
    fn tip(&self) -> wallet_library::Result<BlockId> {
        self.check()?;
        let s = self.0.borrow();
        Ok(BlockId {
            height: (s.blocks.len() - 1) as u32,
            hash: s.blocks.last().unwrap().block_hash(),
        })
    }

    fn block_hash(&self, height: u32) -> wallet_library::Result<BlockHash> {
        self.check()?;
        let s = self.0.borrow();
        s.blocks
            .get(height as usize)
            .map(|b| b.block_hash())
            .ok_or_else(|| WalletError::Backend(format!("no block at height {height}")))
    }

    fn block(&self, hash: &BlockHash) -> wallet_library::Result<Block> {
        self.check()?;
        let s = self.0.borrow();
        s.blocks
            .iter()
            .find(|b| b.block_hash() == *hash)
            .cloned()
            .ok_or_else(|| WalletError::Backend(format!("unknown block {hash}")))
    }

    fn mempool(&self) -> wallet_library::Result<Vec<Transaction>> {
        self.check()?;
        Ok(self.0.borrow().mempool.clone())
    }

    fn broadcast(&self, tx: &Transaction) -> wallet_library::Result<Txid> {
        self.check()?;
        // Like a node accepting a replacement: drop mempool txs it conflicts with.
        let spends: Vec<OutPoint> = tx.input.iter().map(|i| i.previous_output).collect();
        self.0
            .borrow_mut()
            .mempool
            .retain(|m| !m.input.iter().any(|i| spends.contains(&i.previous_output)));
        self.add_to_mempool(tx.clone());
        Ok(tx.compute_txid())
    }

    fn estimate_fee(&self, _target_blocks: u16) -> wallet_library::Result<FeeRate> {
        self.check()?;
        self.0
            .borrow()
            .fee_rate
            .ok_or_else(|| WalletError::FeeEstimation("insufficient data".into()))
    }
}

/// A transaction from outside the wallet paying `sats` to `address`.
/// `tag` makes otherwise-identical payments distinct.
pub fn payment(address: &Address, sats: u64, tag: u32) -> Transaction {
    Transaction {
        version: TxVersion::TWO,
        lock_time: absolute::LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::new(Txid::from_byte_array([tag as u8 + 1; 32]), tag),
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(sats),
            script_pubkey: address.script_pubkey(),
        }],
    }
}
