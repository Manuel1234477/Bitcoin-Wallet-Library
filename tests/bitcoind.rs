//! `BitcoindRpc` + `sync()` against a real regtest Bitcoin Core node.
//!
//! The `bitcoind` dev-dependency downloads Bitcoin Core on first build and
//! each test starts its own node, so tests are isolated.

use bitcoincore_rpc::RpcApi;
use wallet_library::{
    Amount, Auth, BitcoindRpc, ChainBackend, Confirmation, FeeRate, Network, Recipient,
    Transaction, Wallet, WalletError,
};

const MNEMONIC: &str = "abandon abandon abandon abandon abandon abandon \
                        abandon abandon abandon abandon abandon about";

/// A fresh regtest node whose own wallet has 101 blocks' worth of spendable coins.
fn funded_node() -> bitcoind::BitcoinD {
    let node = bitcoind::BitcoinD::from_downloaded().expect("start bitcoind");
    let miner = node.client.get_new_address(None, None).unwrap().assume_checked();
    node.client.generate_to_address(101, &miner).unwrap();
    node
}

fn backend(node: &bitcoind::BitcoinD) -> BitcoindRpc {
    BitcoindRpc::new(&node.rpc_url(), Auth::CookieFile(node.params.cookie_file.clone())).unwrap()
}

fn mine(node: &bitcoind::BitcoinD, blocks: u64) {
    let miner = node.client.get_new_address(None, None).unwrap().assume_checked();
    node.client.generate_to_address(blocks, &miner).unwrap();
}

fn send(node: &bitcoind::BitcoinD, to: &wallet_library::Address, amount: Amount) {
    node.client
        .send_to_address(to, amount, None, None, None, None, None, None)
        .unwrap();
}

#[test]
fn backend_reports_node_tip() {
    let node = funded_node();
    let tip = backend(&node).tip().unwrap();
    assert_eq!(tip.height, 101);
    assert_eq!(tip.hash, node.client.get_best_block_hash().unwrap());
}

#[test]
fn receive_unconfirmed_then_confirmed() {
    let node = funded_node();
    let mut wallet = Wallet::from_mnemonic(MNEMONIC, None, Network::Regtest)
        .unwrap()
        .with_backend(backend(&node));
    let addr = wallet.new_address().unwrap().address;

    send(&node, &addr, Amount::ONE_BTC);
    let summary = wallet.sync().unwrap();
    assert_eq!(summary.tip_height, 101);
    assert_eq!(summary.blocks_applied, 101);
    let balance = wallet.balance();
    assert_eq!(balance.untrusted_pending, Amount::ONE_BTC);
    assert_eq!(balance.confirmed, Amount::ZERO);

    mine(&node, 1);
    wallet.sync().unwrap();
    assert_eq!(wallet.balance().confirmed, Amount::ONE_BTC);
    let utxos = wallet.list_utxos();
    assert_eq!(utxos.len(), 1);
    assert!(matches!(utxos[0].confirmation, Confirmation::Confirmed { height: 102, .. }));
}

#[test]
fn restarted_wallet_recovers_funds_by_rescanning() {
    let node = funded_node();
    {
        let mut wallet = Wallet::from_mnemonic(MNEMONIC, None, Network::Regtest)
            .unwrap()
            .with_backend(backend(&node));
        let addr = wallet.new_address().unwrap().address;
        send(&node, &addr, Amount::from_sat(250_000));
        mine(&node, 1);
        wallet.sync().unwrap();
        assert_eq!(wallet.balance().confirmed, Amount::from_sat(250_000));
    }

    // Nothing is saved: a new wallet rescans the chain and finds the payment again.
    let mut wallet = Wallet::from_mnemonic(MNEMONIC, None, Network::Regtest)
        .unwrap()
        .with_backend(backend(&node));
    assert_eq!(wallet.tip_height(), 0);
    wallet.sync().unwrap();
    assert_eq!(wallet.balance().confirmed, Amount::from_sat(250_000));
}

#[test]
fn build_sign_broadcast_is_accepted_by_node() {
    let node = funded_node();
    let mut wallet = Wallet::from_mnemonic(MNEMONIC, None, Network::Regtest)
        .unwrap()
        .with_backend(backend(&node));
    let addr = wallet.new_address().unwrap().address;
    send(&node, &addr, Amount::ONE_BTC);
    mine(&node, 1);
    wallet.sync().unwrap();

    let to = node.client.get_new_address(None, None).unwrap().assume_checked();
    let amount = Amount::from_sat(30_000_000);
    let fee_rate = FeeRate::from_sat_per_vb(2).unwrap();
    let mut psbt = wallet.build_tx(&[Recipient::new(to, amount)], fee_rate).unwrap();
    let fee = psbt.fee().unwrap();
    assert!(wallet.sign(&mut psbt).unwrap());
    let tx = psbt.extract_tx().unwrap();

    let txid = wallet.broadcast(&tx).unwrap();
    assert!(node.client.get_raw_mempool().unwrap().contains(&txid));
    let change = Amount::ONE_BTC - amount - fee;
    assert_eq!(wallet.balance().trusted_pending, change);

    mine(&node, 1);
    wallet.sync().unwrap();
    assert_eq!(wallet.balance().confirmed, change);
    let tip = node.client.get_best_block_hash().unwrap();
    let block = node.client.get_block(&tip).unwrap();
    assert!(block.txdata.iter().any(|t| t.compute_txid() == txid));
}

#[test]
fn node_accepts_fee_bump_replacement() {
    let node = funded_node();
    let mut wallet = Wallet::from_mnemonic(MNEMONIC, None, Network::Regtest)
        .unwrap()
        .with_backend(backend(&node));
    let addr = wallet.new_address().unwrap().address;
    send(&node, &addr, Amount::ONE_BTC);
    mine(&node, 1);
    wallet.sync().unwrap();

    let to = node.client.get_new_address(None, None).unwrap().assume_checked();
    let low = FeeRate::from_sat_per_vb(1).unwrap();
    let mut psbt = wallet.build_tx(&[Recipient::new(to, Amount::from_sat(30_000_000))], low).unwrap();
    wallet.sign(&mut psbt).unwrap();
    let original = wallet.broadcast(&psbt.extract_tx().unwrap()).unwrap();

    let high = FeeRate::from_sat_per_vb(5).unwrap();
    let mut bump = wallet.bump_fee(original, high).unwrap();
    let fee = bump.fee().unwrap();
    wallet.sign(&mut bump).unwrap();
    let replacement = wallet.broadcast(&bump.extract_tx().unwrap()).unwrap();

    let mempool = node.client.get_raw_mempool().unwrap();
    assert!(mempool.contains(&replacement));
    assert!(!mempool.contains(&original), "node evicted the original");

    mine(&node, 1);
    wallet.sync().unwrap();
    assert_eq!(wallet.balance().confirmed, Amount::ONE_BTC - Amount::from_sat(30_000_000) - fee);
}

#[test]
fn fresh_node_has_no_fee_estimate() {
    let node = funded_node();
    let wallet = Wallet::from_mnemonic(MNEMONIC, None, Network::Regtest)
        .unwrap()
        .with_backend(backend(&node));
    // Core needs to see many transactions confirm before it estimates.
    assert!(matches!(wallet.estimate_fee(6), Err(WalletError::FeeEstimation(_))));
}

#[test]
fn broadcast_rejects_invalid_transaction() {
    let node = funded_node();
    let empty = Transaction {
        version: bdk_wallet::bitcoin::transaction::Version::TWO,
        lock_time: bdk_wallet::bitcoin::absolute::LockTime::ZERO,
        input: vec![],
        output: vec![],
    };
    assert!(matches!(backend(&node).broadcast(&empty), Err(WalletError::Backend(_))));
}

#[test]
fn unreachable_node_is_a_backend_error() {
    let rpc = BitcoindRpc::new("http://127.0.0.1:1", Auth::None).unwrap();
    let mut wallet = Wallet::from_mnemonic(MNEMONIC, None, Network::Regtest)
        .unwrap()
        .with_backend(rpc);
    assert!(matches!(wallet.sync(), Err(WalletError::Backend(_))));
}
