//! `sync()`, `balance()` and `list_utxos()` against an in-memory chain backend.
//! No node required.

mod common;

use common::{MNEMONIC, MockChain, payment};
use wallet_library::{Amount, Confirmation, KeychainKind, Network, OutPoint, Wallet, WalletError};

fn wallet_with(chain: &MockChain) -> Wallet<MockChain> {
    Wallet::from_mnemonic(MNEMONIC, None, Network::Regtest)
        .unwrap()
        .with_backend(chain.clone())
}

#[test]
fn sync_finds_confirmed_payment() {
    let chain = MockChain::new();
    let mut wallet = wallet_with(&chain);
    let addr = wallet.new_address().unwrap().address;

    chain.mine_empty(3);
    let tx = payment(&addr, 50_000, 0);
    chain.mine(vec![tx.clone()]);

    let summary = wallet.sync().unwrap();
    assert_eq!(summary.tip_height, 4);
    assert_eq!(summary.blocks_applied, 4);
    assert_eq!(wallet.tip_height(), 4);
    assert_eq!(wallet.balance().confirmed, Amount::from_sat(50_000));

    let utxos = wallet.list_utxos();
    assert_eq!(utxos.len(), 1);
    assert_eq!(utxos[0].outpoint, OutPoint::new(tx.compute_txid(), 0));
    assert_eq!(utxos[0].keychain, KeychainKind::External);
    assert_eq!(utxos[0].derivation_index, 0);
    assert!(matches!(utxos[0].confirmation, Confirmation::Confirmed { height: 4, .. }));

    let txs = wallet.transactions();
    assert_eq!(txs.len(), 1);
    assert_eq!(txs[0].received, Amount::from_sat(50_000));
    assert_eq!(txs[0].sent, Amount::ZERO);
    assert_eq!(txs[0].fee, None, "inputs aren't ours, so the fee is unknown");
}

#[test]
fn mempool_payment_is_unconfirmed_until_mined() {
    let chain = MockChain::new();
    let mut wallet = wallet_with(&chain);
    let addr = wallet.new_address().unwrap().address;

    let tx = payment(&addr, 20_000, 0);
    chain.add_to_mempool(tx.clone());
    wallet.sync().unwrap();
    let balance = wallet.balance();
    assert_eq!(balance.confirmed, Amount::ZERO);
    assert_eq!(balance.untrusted_pending, Amount::from_sat(20_000));
    assert!(matches!(
        wallet.list_utxos()[0].confirmation,
        Confirmation::Unconfirmed { last_seen: Some(_) }
    ));

    chain.mine(vec![tx]);
    wallet.sync().unwrap();
    let balance = wallet.balance();
    assert_eq!(balance.confirmed, Amount::from_sat(20_000));
    assert_eq!(balance.untrusted_pending, Amount::ZERO);
}

#[test]
fn evicted_mempool_payment_is_dropped() {
    let chain = MockChain::new();
    let mut wallet = wallet_with(&chain);
    let addr = wallet.new_address().unwrap().address;

    chain.add_to_mempool(payment(&addr, 20_000, 0));
    wallet.sync().unwrap();
    assert_eq!(wallet.balance().total(), Amount::from_sat(20_000));

    chain.clear_mempool();
    wallet.sync().unwrap();
    assert_eq!(wallet.balance().total(), Amount::ZERO);
    assert!(wallet.list_utxos().is_empty());
    assert!(wallet.transactions().is_empty());
}

#[test]
fn change_address_payments_are_counted() {
    let chain = MockChain::new();
    let mut wallet = wallet_with(&chain);
    let receive = wallet.new_address().unwrap().address;
    let change = wallet.new_change_address().unwrap().address;

    chain.mine(vec![payment(&receive, 10_000, 0), payment(&change, 5_000, 1)]);
    wallet.sync().unwrap();

    assert_eq!(wallet.balance().confirmed, Amount::from_sat(15_000));
    let mut keychains: Vec<_> = wallet.list_utxos().iter().map(|u| u.keychain).collect();
    keychains.sort();
    assert_eq!(keychains, [KeychainKind::External, KeychainKind::Internal]);
}

#[test]
fn unrevealed_addresses_within_lookahead_are_found() {
    let chain = MockChain::new();
    let mut wallet = wallet_with(&chain);
    // Address #5 is derived by another app using the same seed; this wallet
    // has revealed none yet but scans ahead (BDK's default lookahead is 25).
    let mut other = Wallet::from_mnemonic(MNEMONIC, None, Network::Regtest).unwrap();
    let addr = (0..6).map(|_| other.new_address().unwrap()).last().unwrap().address;

    chain.mine(vec![payment(&addr, 7_000, 0)]);
    wallet.sync().unwrap();
    assert_eq!(wallet.balance().confirmed, Amount::from_sat(7_000));
    assert_eq!(wallet.list_utxos()[0].derivation_index, 5);
    // Seeing the payment marks indices up to 5 as used.
    assert_eq!(wallet.next_derivation_index(KeychainKind::External), 6);
}

#[test]
fn second_sync_is_incremental() {
    let chain = MockChain::new();
    let mut wallet = wallet_with(&chain);
    chain.mine_empty(5);
    assert_eq!(wallet.sync().unwrap().blocks_applied, 5);

    let summary = wallet.sync().unwrap();
    assert_eq!(summary.blocks_applied, 0);
    assert_eq!(summary.fork_height, 5);

    chain.mine_empty(2);
    let summary = wallet.sync().unwrap();
    assert_eq!(summary.blocks_applied, 2);
    assert_eq!(summary.tip_height, 7);
}

#[test]
fn reorg_unconfirms_payment() {
    let chain = MockChain::new();
    let mut wallet = wallet_with(&chain);
    let addr = wallet.new_address().unwrap().address;

    chain.mine_empty(1);
    let tx = payment(&addr, 30_000, 0);
    chain.mine(vec![tx.clone()]); // height 2
    chain.mine_empty(1); // height 3
    wallet.sync().unwrap();
    assert_eq!(wallet.balance().confirmed, Amount::from_sat(30_000));

    // Replace heights 2..=3 with a longer branch that doesn't include `tx`;
    // the tx returns to the mempool.
    chain.disconnect_from(2);
    chain.mine_empty(3); // heights 2..=4
    chain.add_to_mempool(tx);
    let summary = wallet.sync().unwrap();

    assert_eq!(summary.fork_height, 1);
    assert_eq!(summary.blocks_applied, 3);
    assert_eq!(wallet.tip_height(), 4);
    let balance = wallet.balance();
    assert_eq!(balance.confirmed, Amount::ZERO);
    assert_eq!(balance.untrusted_pending, Amount::from_sat(30_000));
}

#[test]
fn birthday_skips_older_blocks() {
    let chain = MockChain::new();
    let mut wallet = wallet_with(&chain).with_birthday(8);
    chain.mine_empty(10);

    let summary = wallet.sync().unwrap();
    assert_eq!(summary.blocks_applied, 3, "heights 8, 9 and 10");
    assert_eq!(wallet.tip_height(), 10);
}

#[test]
fn wrong_network_backend_is_rejected() {
    let chain = MockChain::new(); // regtest genesis
    let mut wallet = Wallet::from_mnemonic(MNEMONIC, None, Network::Testnet)
        .unwrap()
        .with_backend(chain);
    assert!(matches!(wallet.sync(), Err(WalletError::Sync(_))));
}

#[test]
fn backend_errors_are_reported() {
    let chain = MockChain::new();
    let mut wallet = wallet_with(&chain);
    chain.set_failing(true);
    assert!(matches!(wallet.sync(), Err(WalletError::Backend(_))));
}

#[test]
fn restarted_wallet_recovers_funds_by_rescanning() {
    let chain = MockChain::new();
    let mut wallet = wallet_with(&chain);
    let addr = wallet.new_address().unwrap();
    chain.mine_empty(2);
    chain.mine(vec![payment(&addr.address, 40_000, 0)]);
    wallet.sync().unwrap();
    drop(wallet);

    // Nothing is saved, so a new wallet from the same mnemonic starts empty...
    let mut restarted = wallet_with(&chain);
    assert_eq!(restarted.tip_height(), 0);
    assert_eq!(restarted.balance().confirmed, Amount::ZERO);
    // ...and rebuilds its state from the chain.
    assert_eq!(restarted.sync().unwrap().blocks_applied, 3);
    assert_eq!(restarted.balance().confirmed, Amount::from_sat(40_000));
    assert_eq!(restarted.list_utxos().len(), 1);
    // The address that received funds is not handed out again.
    assert_ne!(restarted.new_address().unwrap(), addr);
}
