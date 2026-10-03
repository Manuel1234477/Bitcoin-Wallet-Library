//! Transaction building, fee bumping and UTXO reservation.
//!
//! Recipients are checked here (non-empty, right network); coin selection,
//! change and fee calculation are delegated to BDK's `TxBuilder`. The result
//! is an unsigned PSBT, so building and signing can happen on different
//! devices (e.g. a watch-only wallet builds, an offline wallet signs).
//!
//! Every built PSBT **reserves** its inputs (BDK outpoint locks), so the next
//! build can't select them too. A reservation ends when the transaction is
//! broadcast (the inputs are spent) or cancelled with [`cancel`].

use std::collections::HashSet;

use bdk_wallet::KeychainKind;
use bdk_wallet::bitcoin::{Address, Amount, FeeRate, Network, OutPoint, Psbt, Transaction, Txid};
use bdk_wallet::error::{BuildFeeBumpError, CreateTxError};

use crate::error::{Result, WalletError};

/// A payment: send `amount` to `address`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipient {
    pub address: Address,
    pub amount: Amount,
}

impl Recipient {
    pub fn new(address: Address, amount: Amount) -> Self {
        Self { address, amount }
    }
}

impl From<(Address, Amount)> for Recipient {
    fn from((address, amount): (Address, Amount)) -> Self {
        Self { address, amount }
    }
}

/// Build an unsigned PSBT paying `recipients`, with change back to the wallet.
pub(crate) fn build(
    wallet: &mut bdk_wallet::Wallet,
    recipients: &[Recipient],
    fee_rate: FeeRate,
) -> Result<Psbt> {
    if recipients.is_empty() {
        return Err(WalletError::InvalidRecipient("no recipients".into()));
    }
    for recipient in recipients {
        check_network(&recipient.address, wallet.network())?;
    }

    let mut builder = wallet.build_tx();
    for recipient in recipients {
        builder.add_recipient(recipient.address.script_pubkey(), recipient.amount);
    }
    builder.fee_rate(fee_rate);
    let psbt = builder.finish().map_err(map_create_tx_error)?;
    reserve(wallet, &psbt.unsigned_tx);
    Ok(psbt)
}

/// Build an unsigned PSBT sending every wallet UTXO to `address`, minus the fee.
pub(crate) fn build_drain(
    wallet: &mut bdk_wallet::Wallet,
    address: &Address,
    fee_rate: FeeRate,
) -> Result<Psbt> {
    check_network(address, wallet.network())?;

    let mut builder = wallet.build_tx();
    builder
        .drain_wallet()
        .drain_to(address.script_pubkey())
        .fee_rate(fee_rate);
    let psbt = builder.finish().map_err(map_create_tx_error)?;
    reserve(wallet, &psbt.unsigned_tx);
    Ok(psbt)
}

/// Build an unsigned replacement (RBF) for the unconfirmed wallet transaction
/// `txid`, paying `fee_rate`. Recipients stay the same; the extra fee comes
/// from the change output, or from extra inputs if the change is too small.
pub(crate) fn bump(wallet: &mut bdk_wallet::Wallet, txid: Txid, fee_rate: FeeRate) -> Result<Psbt> {
    let mut builder = wallet.build_fee_bump(txid).map_err(map_fee_bump_error)?;
    builder.fee_rate(fee_rate);
    let psbt = builder.finish().map_err(map_create_tx_error)?;
    reserve(wallet, &psbt.unsigned_tx);
    Ok(psbt)
}

/// Release a built-but-unsent transaction: unlock its inputs and let its
/// change address be handed out again.
pub(crate) fn cancel(wallet: &mut bdk_wallet::Wallet, tx: &Transaction) {
    release(wallet, tx);
    for output in &tx.output {
        if let Some((KeychainKind::Internal, index)) = wallet.derivation_of_spk(output.script_pubkey.clone()) {
            wallet.unmark_used(KeychainKind::Internal, index);
        }
    }
}

/// Record a transaction the backend accepted: add it as unconfirmed and evict
/// the unconfirmed wallet transactions it replaces (those spending any of the
/// same outputs), since the node has dropped them from its mempool.
pub(crate) fn record_broadcast(wallet: &mut bdk_wallet::Wallet, tx: &Transaction, seen_at: u64) {
    let txid = tx.compute_txid();
    let spends: HashSet<OutPoint> = tx.input.iter().map(|i| i.previous_output).collect();
    let replaced: Vec<(Txid, u64)> = wallet
        .transactions()
        .filter(|wtx| !wtx.chain_position.is_confirmed() && wtx.tx_node.txid != txid)
        .filter(|wtx| wtx.tx_node.tx.input.iter().any(|i| spends.contains(&i.previous_output)))
        .map(|wtx| (wtx.tx_node.txid, seen_at))
        .collect();

    wallet.apply_unconfirmed_txs([(tx.clone(), seen_at)]);
    wallet.apply_evicted_txs(replaced);
    release(wallet, tx);
}

fn reserve(wallet: &mut bdk_wallet::Wallet, tx: &Transaction) {
    for input in &tx.input {
        wallet.lock_outpoint(input.previous_output);
    }
}

fn release(wallet: &mut bdk_wallet::Wallet, tx: &Transaction) {
    for input in &tx.input {
        wallet.unlock_outpoint(input.previous_output);
    }
}

/// `Address` parsing only checks the network when the caller asks for it, so
/// re-check here: paying a testnet address from a mainnet wallet would burn coins.
fn check_network(address: &Address, network: Network) -> Result<()> {
    if address.as_unchecked().is_valid_for_network(network) {
        Ok(())
    } else {
        Err(WalletError::InvalidRecipient(format!(
            "address {address} is not valid for {network}"
        )))
    }
}

fn map_create_tx_error(err: CreateTxError) -> WalletError {
    match err {
        CreateTxError::CoinSelection(e) => WalletError::InsufficientFunds {
            needed: e.needed,
            available: e.available,
        },
        CreateTxError::NoRecipients => WalletError::InvalidRecipient("no recipients".into()),
        CreateTxError::OutputBelowDustLimit(index) => {
            WalletError::InvalidRecipient(format!("output {index} is below the dust limit"))
        }
        CreateTxError::FeeRateTooLow { required } => WalletError::FeeRateTooLow { required },
        CreateTxError::FeeTooLow { required } => {
            WalletError::FeeBump(format!("replacement must pay an absolute fee of at least {required}"))
        }
        other => WalletError::TxBuild(other.to_string()),
    }
}

fn map_fee_bump_error(err: BuildFeeBumpError) -> WalletError {
    match err {
        BuildFeeBumpError::TransactionNotFound(txid) => {
            WalletError::FeeBump(format!("transaction {txid} is not in the wallet"))
        }
        BuildFeeBumpError::TransactionConfirmed(txid) => {
            WalletError::FeeBump(format!("transaction {txid} is already confirmed"))
        }
        BuildFeeBumpError::IrreplaceableTransaction(txid) => {
            WalletError::FeeBump(format!("transaction {txid} does not signal replaceability (RBF)"))
        }
        other => WalletError::FeeBump(other.to_string()),
    }
}
