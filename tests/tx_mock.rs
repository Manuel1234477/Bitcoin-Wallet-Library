//! `build_tx()`, `sign()` and `broadcast()` against an in-memory chain backend.
//! No node required.

mod common;

use bdk_wallet::bitcoin::Weight;
use common::{MNEMONIC, MockChain, payment};
use wallet_library::{
    Address, Amount, FeeRate, KeychainKind, Network, Recipient, Wallet, WalletError,
};

/// A different wallet to pay to.
const OTHER_MNEMONIC: &str = "zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo wrong";

const FUNDS: u64 = 100_000;

fn fee_rate() -> FeeRate {
    FeeRate::from_sat_per_vb(5).unwrap()
}

fn other_address(network: Network) -> Address {
    Wallet::from_mnemonic(OTHER_MNEMONIC, None, network)
        .unwrap()
        .new_address()
        .unwrap()
        .address
}

/// A synced wallet holding one confirmed `FUNDS` UTXO.
fn funded_wallet(chain: &MockChain) -> Wallet<MockChain> {
    let mut wallet = Wallet::from_mnemonic(MNEMONIC, None, Network::Regtest)
        .unwrap()
        .with_backend(chain.clone());
    let addr = wallet.new_address().unwrap().address;
    chain.mine(vec![payment(&addr, FUNDS, 0)]);
    wallet.sync().unwrap();
    wallet
}

#[test]
fn build_tx_pays_recipient_and_returns_change() {
    let chain = MockChain::new();
    let mut wallet = funded_wallet(&chain);
    let to = other_address(Network::Regtest);

    let psbt = wallet
        .build_tx(&[Recipient::new(to.clone(), Amount::from_sat(30_000))], fee_rate())
        .unwrap();

    let tx = &psbt.unsigned_tx;
    assert_eq!(tx.input.len(), 1);
    assert_eq!(tx.output.len(), 2);
    let payment = tx.output.iter().find(|o| o.script_pubkey == to.script_pubkey()).unwrap();
    assert_eq!(payment.value, Amount::from_sat(30_000));

    let fee = psbt.fee().unwrap();
    let change = tx.output.iter().find(|o| o.script_pubkey != to.script_pubkey()).unwrap();
    assert_eq!(change.value, Amount::from_sat(FUNDS - 30_000) - fee);
    // Change goes to the wallet's internal keychain.
    assert_eq!(wallet.next_derivation_index(KeychainKind::Internal), 1);
    assert!(psbt.inputs.iter().all(|i| i.final_script_witness.is_none()), "unsigned");
}

#[test]
fn signed_tx_is_final_and_pays_the_requested_fee_rate() {
    let chain = MockChain::new();
    let mut wallet = funded_wallet(&chain);
    let mut psbt = wallet
        .build_tx(&[(other_address(Network::Regtest), Amount::from_sat(30_000)).into()], fee_rate())
        .unwrap();
    let fee = psbt.fee().unwrap();

    assert!(wallet.sign(&mut psbt).unwrap(), "PSBT should be finalized");
    let tx = psbt.extract_tx().unwrap();
    assert!(tx.input.iter().all(|i| !i.witness.is_empty()));

    // BDK estimates the signature at its maximum size, so the real fee rate
    // can only be equal or slightly higher. (Compared by weight: `vsize()`
    // rounds up, which would make the rate look a fraction too low.)
    let weight = tx.weight();
    assert!(fee >= fee_rate().fee_wu(weight).unwrap());
    assert!(fee <= fee_rate().fee_wu(weight + Weight::from_vb_unchecked(1)).unwrap());
}

#[test]
fn broadcast_updates_balance_before_and_after_confirmation() {
    let chain = MockChain::new();
    let mut wallet = funded_wallet(&chain);
    let mut psbt = wallet
        .build_tx(&[Recipient::new(other_address(Network::Regtest), Amount::from_sat(30_000))], fee_rate())
        .unwrap();
    let fee = psbt.fee().unwrap();
    wallet.sign(&mut psbt).unwrap();
    let tx = psbt.extract_tx().unwrap();
    let change = Amount::from_sat(FUNDS - 30_000) - fee;

    let txid = wallet.broadcast(&tx).unwrap();
    assert_eq!(txid, tx.compute_txid());
    let balance = wallet.balance();
    assert_eq!(balance.confirmed, Amount::ZERO);
    assert_eq!(balance.trusted_pending, change, "our own change is trusted");
    assert_eq!(wallet.list_utxos().len(), 1);

    let record = wallet.transactions().into_iter().find(|r| r.txid == txid).unwrap();
    assert_eq!(record.sent, Amount::from_sat(FUNDS));
    assert_eq!(record.received, change);
    assert_eq!(record.fee, Some(fee));

    // The mock backend put it in its mempool; a sync keeps it.
    wallet.sync().unwrap();
    assert_eq!(wallet.balance().trusted_pending, change);

    chain.mine(vec![tx]);
    wallet.sync().unwrap();
    let balance = wallet.balance();
    assert_eq!(balance.confirmed, change);
    assert_eq!(balance.trusted_pending, Amount::ZERO);
}

#[test]
fn failed_broadcast_leaves_wallet_unchanged() {
    let chain = MockChain::new();
    let mut wallet = funded_wallet(&chain);
    let mut psbt = wallet
        .build_tx(&[Recipient::new(other_address(Network::Regtest), Amount::from_sat(30_000))], fee_rate())
        .unwrap();
    wallet.sign(&mut psbt).unwrap();

    chain.set_failing(true);
    let result = wallet.broadcast(&psbt.extract_tx().unwrap());
    assert!(matches!(result, Err(WalletError::Backend(_))));
    assert_eq!(wallet.balance().confirmed, Amount::from_sat(FUNDS));
    assert_eq!(wallet.transactions().len(), 1);
}

#[test]
fn drain_sends_everything_with_no_change() {
    let chain = MockChain::new();
    let mut wallet = funded_wallet(&chain);
    let addr = wallet.new_address().unwrap().address;
    chain.mine(vec![payment(&addr, 25_000, 1)]);
    wallet.sync().unwrap();
    let to = other_address(Network::Regtest);

    let mut psbt = wallet.build_drain_tx(&to, fee_rate()).unwrap();
    let fee = psbt.fee().unwrap();
    assert_eq!(psbt.unsigned_tx.input.len(), 2);
    assert_eq!(psbt.unsigned_tx.output.len(), 1);
    assert_eq!(psbt.unsigned_tx.output[0].script_pubkey, to.script_pubkey());
    assert_eq!(psbt.unsigned_tx.output[0].value, Amount::from_sat(FUNDS + 25_000) - fee);

    assert!(wallet.sign(&mut psbt).unwrap());
    wallet.broadcast(&psbt.extract_tx().unwrap()).unwrap();
    assert_eq!(wallet.balance().total(), Amount::ZERO);
    assert!(wallet.list_utxos().is_empty());
}

#[test]
fn insufficient_funds_reports_amounts() {
    let chain = MockChain::new();
    let mut wallet = funded_wallet(&chain);
    let err = wallet
        .build_tx(&[Recipient::new(other_address(Network::Regtest), Amount::from_sat(FUNDS))], fee_rate())
        .unwrap_err();
    match err {
        WalletError::InsufficientFunds { needed, available } => {
            assert_eq!(available, Amount::from_sat(FUNDS));
            assert!(needed > Amount::from_sat(FUNDS), "amount plus fee");
        }
        other => panic!("expected InsufficientFunds, got {other:?}"),
    }
}

#[test]
fn empty_wallet_cannot_build() {
    let chain = MockChain::new();
    let mut wallet = Wallet::from_mnemonic(MNEMONIC, None, Network::Regtest)
        .unwrap()
        .with_backend(chain);
    let result = wallet.build_tx(
        &[Recipient::new(other_address(Network::Regtest), Amount::from_sat(10_000))],
        fee_rate(),
    );
    assert!(matches!(result, Err(WalletError::InsufficientFunds { .. })));
}

#[test]
fn invalid_recipients_are_rejected() {
    let chain = MockChain::new();
    let mut wallet = funded_wallet(&chain);

    let no_recipients = wallet.build_tx(&[], fee_rate());
    assert!(matches!(no_recipients, Err(WalletError::InvalidRecipient(_))));

    let dust = wallet.build_tx(
        &[Recipient::new(other_address(Network::Regtest), Amount::from_sat(100))],
        fee_rate(),
    );
    assert!(matches!(dust, Err(WalletError::InvalidRecipient(_))));

    let mainnet = other_address(Network::Bitcoin);
    let wrong_network = wallet.build_tx(&[Recipient::new(mainnet.clone(), Amount::from_sat(10_000))], fee_rate());
    assert!(matches!(wrong_network, Err(WalletError::InvalidRecipient(_))));
    let wrong_network = wallet.build_drain_tx(&mainnet, fee_rate());
    assert!(matches!(wrong_network, Err(WalletError::InvalidRecipient(_))));
}

#[test]
fn watch_only_wallet_builds_and_full_wallet_signs() {
    let chain = MockChain::new();
    let full = funded_wallet(&chain);
    assert!(!full.is_watch_only());

    let mut watch_only = Wallet::from_descriptor(
        &full.public_descriptor(KeychainKind::External),
        Some(&full.public_descriptor(KeychainKind::Internal)),
        Network::Regtest,
    )
    .unwrap()
    .with_backend(chain.clone());
    assert!(watch_only.is_watch_only());
    watch_only.sync().unwrap();

    let mut psbt = watch_only
        .build_tx(&[Recipient::new(other_address(Network::Regtest), Amount::from_sat(30_000))], fee_rate())
        .unwrap();
    assert_eq!(watch_only.sign(&mut psbt), Err(WalletError::WatchOnly));

    // Hand the PSBT to the wallet holding the keys.
    assert!(full.sign(&mut psbt).unwrap());
    let txid = watch_only.broadcast(&psbt.extract_tx().unwrap()).unwrap();
    assert!(watch_only.transactions().iter().any(|r| r.txid == txid));
}

#[test]
fn estimated_fee_rate_is_used_for_building() {
    let chain = MockChain::new();
    let mut wallet = funded_wallet(&chain);
    chain.set_fee_rate(Some(FeeRate::from_sat_per_vb(12).unwrap()));

    let rate = wallet.estimate_fee(6).unwrap();
    assert_eq!(rate, FeeRate::from_sat_per_vb(12).unwrap());
    let mut psbt = wallet
        .build_tx(&[Recipient::new(other_address(Network::Regtest), Amount::from_sat(30_000))], rate)
        .unwrap();
    let fee = psbt.fee().unwrap();
    wallet.sign(&mut psbt).unwrap();
    assert!(fee >= rate.fee_wu(psbt.extract_tx().unwrap().weight()).unwrap());
}

#[test]
fn estimate_never_goes_below_relay_minimum() {
    let chain = MockChain::new();
    let wallet = funded_wallet(&chain);
    chain.set_fee_rate(Some(FeeRate::from_sat_per_kwu(100))); // 0.4 sat/vB
    assert_eq!(wallet.estimate_fee(1).unwrap(), FeeRate::BROADCAST_MIN);
}

#[test]
fn estimate_errors() {
    let chain = MockChain::new();
    let wallet = funded_wallet(&chain);

    assert!(matches!(wallet.estimate_fee(6), Err(WalletError::FeeEstimation(_))), "no data");

    chain.set_fee_rate(Some(FeeRate::from_sat_per_vb(5).unwrap()));
    assert!(matches!(wallet.estimate_fee(0), Err(WalletError::FeeEstimation(_))), "zero target");

    chain.set_failing(true);
    assert!(matches!(wallet.estimate_fee(6), Err(WalletError::Backend(_))));
}

// --- UTXO reservation ---

/// A synced wallet holding two confirmed UTXOs: `FUNDS` and 60_000 sats.
fn wallet_with_two_utxos(chain: &MockChain) -> Wallet<MockChain> {
    let mut wallet = funded_wallet(chain);
    let addr = wallet.new_address().unwrap().address;
    chain.mine(vec![payment(&addr, 60_000, 1)]);
    wallet.sync().unwrap();
    wallet
}

fn pay(amount: u64) -> [Recipient; 1] {
    [Recipient::new(other_address(Network::Regtest), Amount::from_sat(amount))]
}

#[test]
fn built_tx_reserves_its_inputs() {
    let chain = MockChain::new();
    let mut wallet = wallet_with_two_utxos(&chain);

    let first = wallet.build_tx(&pay(50_000), fee_rate()).unwrap();
    let second = wallet.build_tx(&pay(50_000), fee_rate()).unwrap();
    let first_inputs: Vec<_> = first.unsigned_tx.input.iter().map(|i| i.previous_output).collect();
    assert!(
        second.unsigned_tx.input.iter().all(|i| !first_inputs.contains(&i.previous_output)),
        "second build must not reuse the first build's UTXOs"
    );

    // Both UTXOs are now reserved: nothing is left to spend.
    assert!(wallet.list_utxos().iter().all(|u| u.reserved));
    assert!(matches!(
        wallet.build_tx(&pay(10_000), fee_rate()),
        Err(WalletError::InsufficientFunds { available: Amount::ZERO, .. })
    ));
    // Reserved coins are still the wallet's.
    assert_eq!(wallet.balance().confirmed, Amount::from_sat(FUNDS + 60_000));
}

#[test]
fn cancel_releases_reservation_and_change_address() {
    let chain = MockChain::new();
    let mut wallet = funded_wallet(&chain);
    let psbt = wallet.build_tx(&pay(30_000), fee_rate()).unwrap();
    assert!(wallet.list_utxos()[0].reserved);

    wallet.cancel_tx(&psbt);
    assert!(!wallet.list_utxos()[0].reserved);

    // The UTXO and the change address are both available again.
    let rebuilt = wallet.build_tx(&pay(30_000), fee_rate()).unwrap();
    assert_eq!(rebuilt.unsigned_tx.input, psbt.unsigned_tx.input);
    let change = |p: &wallet_library::Psbt| {
        let to = other_address(Network::Regtest).script_pubkey();
        p.unsigned_tx.output.iter().find(|o| o.script_pubkey != to).unwrap().script_pubkey.clone()
    };
    assert_eq!(change(&rebuilt), change(&psbt));
}

#[test]
fn broadcast_ends_reservation_but_failed_broadcast_keeps_it() {
    let chain = MockChain::new();
    let mut wallet = wallet_with_two_utxos(&chain);
    let mut psbt = wallet.build_tx(&pay(50_000), fee_rate()).unwrap();
    wallet.sign(&mut psbt).unwrap();
    let tx = psbt.extract_tx().unwrap();

    chain.set_failing(true);
    assert!(wallet.broadcast(&tx).is_err());
    assert_eq!(wallet.list_utxos().iter().filter(|u| u.reserved).count(), 1);

    chain.set_failing(false);
    wallet.broadcast(&tx).unwrap();
    assert!(wallet.list_utxos().iter().all(|u| !u.reserved));
}

#[test]
fn drain_skips_reserved_utxos() {
    let chain = MockChain::new();
    let mut wallet = wallet_with_two_utxos(&chain);
    let reserved = wallet.build_tx(&pay(50_000), fee_rate()).unwrap();

    let drain = wallet.build_drain_tx(&other_address(Network::Regtest), fee_rate()).unwrap();
    assert_eq!(drain.unsigned_tx.input.len(), 1);
    assert_ne!(drain.unsigned_tx.input[0].previous_output, reserved.unsigned_tx.input[0].previous_output);
}

// --- Fee bumping (RBF) ---

/// Broadcast a 30_000 sat payment at 2 sat/vB; returns (txid, fee).
fn send_low_fee(wallet: &mut Wallet<MockChain>) -> (wallet_library::Txid, Amount) {
    let mut psbt = wallet
        .build_tx(&pay(30_000), FeeRate::from_sat_per_vb(2).unwrap())
        .unwrap();
    let fee = psbt.fee().unwrap();
    wallet.sign(&mut psbt).unwrap();
    (wallet.broadcast(&psbt.extract_tx().unwrap()).unwrap(), fee)
}

#[test]
fn bump_fee_replaces_unconfirmed_tx() {
    let chain = MockChain::new();
    let mut wallet = funded_wallet(&chain);
    let (original, old_fee) = send_low_fee(&mut wallet);
    let to = other_address(Network::Regtest).script_pubkey();

    let mut psbt = wallet.bump_fee(original, FeeRate::from_sat_per_vb(10).unwrap()).unwrap();
    let new_fee = psbt.fee().unwrap();
    assert!(new_fee > old_fee);
    let payment = psbt.unsigned_tx.output.iter().find(|o| o.script_pubkey == to).unwrap();
    assert_eq!(payment.value, Amount::from_sat(30_000), "recipient is paid the same");

    assert!(wallet.sign(&mut psbt).unwrap());
    let replacement = psbt.extract_tx().unwrap();
    let replacement_id = wallet.broadcast(&replacement).unwrap();

    // Only the replacement remains, and the balance counts its change once.
    let change = Amount::from_sat(FUNDS - 30_000) - new_fee;
    let unconfirmed: Vec<_> = wallet.transactions().into_iter().filter(|r| !r.confirmation.is_confirmed()).collect();
    assert_eq!(unconfirmed.len(), 1);
    assert_eq!(unconfirmed[0].txid, replacement_id);
    assert_eq!(wallet.balance().trusted_pending, change);

    // A sync agrees (the original left the mempool), and mining confirms it.
    wallet.sync().unwrap();
    assert_eq!(wallet.balance().trusted_pending, change);
    chain.mine(vec![replacement]);
    wallet.sync().unwrap();
    assert_eq!(wallet.balance().confirmed, change);
    assert!(wallet.transactions().iter().all(|r| r.txid != original));
}

#[test]
fn bump_fee_rejects_lower_or_equal_rate() {
    let chain = MockChain::new();
    let mut wallet = funded_wallet(&chain);
    let (original, _) = send_low_fee(&mut wallet);
    let result = wallet.bump_fee(original, FeeRate::from_sat_per_vb(2).unwrap());
    assert!(matches!(result, Err(WalletError::FeeRateTooLow { .. })), "{result:?}");
}

#[test]
fn bump_fee_rejects_unknown_and_confirmed_txs() {
    let chain = MockChain::new();
    let mut wallet = funded_wallet(&chain);
    let rate = FeeRate::from_sat_per_vb(10).unwrap();

    let unknown = wallet_library::Txid::from_raw_hash(bdk_wallet::bitcoin::hashes::Hash::all_zeros());
    assert!(matches!(wallet.bump_fee(unknown, rate), Err(WalletError::FeeBump(_))));

    // The funding payment is already confirmed.
    let funding = wallet.transactions()[0].txid;
    assert!(matches!(wallet.bump_fee(funding, rate), Err(WalletError::FeeBump(_))));
}

#[test]
fn cancelled_bump_leaves_original_in_place() {
    let chain = MockChain::new();
    let mut wallet = funded_wallet(&chain);
    let (original, old_fee) = send_low_fee(&mut wallet);
    let balance = wallet.balance();

    let psbt = wallet.bump_fee(original, FeeRate::from_sat_per_vb(10).unwrap()).unwrap();
    wallet.cancel_tx(&psbt);

    assert_eq!(wallet.balance(), balance);
    let record = wallet.transactions().into_iter().find(|r| r.txid == original).unwrap();
    assert_eq!(record.fee, Some(old_fee));
    // It can still be bumped later.
    wallet.bump_fee(original, FeeRate::from_sat_per_vb(10).unwrap()).unwrap();
}
