# Bitcoin Wallet Library

A small, focused Bitcoin wallet library for Rust, built on [BDK](https://bitcoindevkit.org/). It is meant as the foundation for wallet apps and CLIs.

- **Wallets from a BIP39 mnemonic or output descriptors**: BIP84 native segwit by default. Public-key descriptors give a watch-only wallet.
- **Pluggable chain backend**: sync goes through the `ChainBackend` trait. A Bitcoin Core JSON-RPC backend is included, and you can implement the trait yourself, for example a mock for tests.
- **Spending**: build unsigned PSBTs, sign, broadcast, drain the wallet, bump fees (RBF) and estimate fee rates.
- **Safety**: recipients are checked against the wallet's network, UTXOs selected by a built transaction are reserved until it's broadcast or cancelled, and `Debug` output never shows private keys.
- **Typed errors**: every failure is a `WalletError` variant your app can match on.

## Installation

The crate isn't on crates.io yet. Add it from GitHub:

```toml
[dependencies]
wallet-library = { git = "https://github.com/Manuel1234477/Bitcoin-Wallet-Library" }
```

Add `rev = "<commit>"` or `tag = "<tag>"` to pin a version. To work against a local checkout, use `path = "/path/to/Bitcoin-Wallet-Library"` instead.

In code the crate is called `wallet_library`. It re-exports the Bitcoin types you need (`Address`, `Amount`, `FeeRate`, `Network`, `Psbt`, `Transaction`, `Txid`, …), so you don't need a direct `bdk_wallet` or `bitcoin` dependency.

Requires Rust 1.85 or newer.

## Usage

### Create a wallet

```rust
use wallet_library::keys::{generate_mnemonic, WordCount};
use wallet_library::{Network, Wallet};

// A new random 12-word mnemonic. Show it to the user to back up; never log it.
let mnemonic = generate_mnemonic(WordCount::Words12)?;
let mut wallet = Wallet::from_mnemonic(&mnemonic.to_string(), None, Network::Regtest)?;

// Or restore from an existing phrase (optionally with a BIP39 passphrase).
let phrase = "abandon abandon abandon abandon abandon abandon \
              abandon abandon abandon abandon abandon about";
let mut wallet = Wallet::from_mnemonic(phrase, None, Network::Regtest)?;

let address = wallet.new_address()?;
println!("receive at {address} (index {})", address.index);
```

`Wallet::from_descriptor(external, Some(internal), network)` creates a wallet from descriptor strings instead.

### Sync and check the balance

```rust
use wallet_library::{Auth, BitcoindRpc};

let backend = BitcoindRpc::new(
    "http://127.0.0.1:18443",
    Auth::CookieFile("/home/me/.bitcoin/regtest/.cookie".into()),
)?;
let mut wallet = wallet.with_backend(backend).with_birthday(0);

let summary = wallet.sync()?;
println!("synced to height {}", summary.tip_height);

let balance = wallet.balance();
println!("confirmed:   {}", balance.confirmed);
println!("unconfirmed: {}", balance.trusted_pending + balance.untrusted_pending);

for utxo in wallet.list_utxos() {
    println!("{} {} reserved={}", utxo.outpoint, utxo.value, utxo.reserved);
}
```

`with_birthday(height)` skips blocks older than the wallet on the first sync. Wallet state is kept in memory only, so a new `Wallet` rescans from its birthday.

### Send a payment

```rust
use std::str::FromStr;
use wallet_library::{Address, Amount, FeeRate, Network, Recipient, WalletError};

let to = Address::from_str("bcrt1qw508d6qejxtdg4y5r3zarvary0c5xw7kygt080")?
    .require_network(Network::Regtest)?;

// Aim to confirm within 6 blocks. Fall back to a fixed rate if the node has no estimate yet.
let fee_rate = match wallet.estimate_fee(6) {
    Ok(rate) => rate,
    Err(WalletError::FeeEstimation(_)) => FeeRate::from_sat_per_vb(2).unwrap(),
    Err(e) => return Err(e.into()),
};

// 1. Build an unsigned PSBT. Coin selection, change and fee are handled for you.
let mut psbt = wallet.build_tx(&[Recipient::new(to, Amount::from_sat(50_000))], fee_rate)?;

// 2. Sign. Returns true once every input is signed and the PSBT is finalized.
let finalized = wallet.sign(&mut psbt)?;
assert!(finalized);

// 3. Broadcast. The wallet records the tx right away, so balance() reflects it.
let tx = psbt.extract_tx()?;
let txid = wallet.broadcast(&tx)?;
println!("sent {txid}");
```

`build_drain_tx(&address, fee_rate)` sends the whole balance to one address with no change.

UTXOs chosen by `build_tx` are **reserved** so the next build can't spend them too. Broadcasting ends the reservation. To discard a transaction you won't send, call `cancel_tx`:

```rust
let psbt = wallet.build_tx(&recipients, fee_rate)?;
// ...the user changes their mind
wallet.cancel_tx(&psbt); // frees its UTXOs and change address
```

### Bump the fee of a stuck transaction

```rust
let mut replacement = wallet.bump_fee(txid, FeeRate::from_sat_per_vb(10).unwrap())?;
wallet.sign(&mut replacement)?;
wallet.broadcast(&replacement.extract_tx()?)?; // the original is dropped from the wallet
```

The recipients are paid the same amounts; the extra fee comes out of the change.

### Watch-only wallets and offline signing

An online watch-only wallet builds the PSBT, and an offline wallet holding the keys signs it:

```rust
use wallet_library::KeychainKind;

let mut watch_only = Wallet::from_descriptor(
    &signer.public_descriptor(KeychainKind::External),
    Some(&signer.public_descriptor(KeychainKind::Internal)),
    Network::Regtest,
)?
.with_backend(backend);
watch_only.sync()?;

let mut psbt = watch_only.build_tx(&recipients, fee_rate)?;
signer.sign(&mut psbt)?; // on the device that has the keys
watch_only.broadcast(&psbt.extract_tx()?)?;
```

Calling `sign` on a watch-only wallet returns `WalletError::WatchOnly`.

### Custom chain backends

Implement `ChainBackend` to sync from another source, or to test without a node:

```rust
use wallet_library::{Block, BlockHash, BlockId, ChainBackend, FeeRate, Result, Transaction, Txid};

struct MyBackend;

impl ChainBackend for MyBackend {
    fn tip(&self) -> Result<BlockId> { todo!() }
    fn block_hash(&self, height: u32) -> Result<BlockHash> { todo!() }
    fn block(&self, hash: &BlockHash) -> Result<Block> { todo!() }
    fn mempool(&self) -> Result<Vec<Transaction>> { todo!() }
    fn broadcast(&self, tx: &Transaction) -> Result<Txid> { todo!() }
    fn estimate_fee(&self, target_blocks: u16) -> Result<FeeRate> { todo!() }
}
```

Report failures as `WalletError::Backend`. Reorg handling lives in the wallet, so a backend is only a data source.

### Errors

Every fallible call returns `wallet_library::Result<T>`. Some of the `WalletError` variants you'll handle most often:

| Variant | When |
|---|---|
| `InsufficientFunds { needed, available }` | Unreserved UTXOs can't cover the amount plus fee |
| `InvalidRecipient` | No recipients, an address for another network, or an amount below the dust limit |
| `FeeEstimation` | The backend has no fee estimate (common on fresh or regtest nodes) |
| `FeeBump` / `FeeRateTooLow { required }` | The tx can't be replaced, or the new rate is too low |
| `WatchOnly` | `sign` called on a wallet without private keys |
| `Backend` / `Sync` | The node failed, or its chain doesn't match the wallet's network |

## Running the tests

```sh
cargo test
```

The mock-backend tests need no setup. The tests in `tests/bitcoind.rs` download Bitcoin Core on first build and start a separate regtest node for each test.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
