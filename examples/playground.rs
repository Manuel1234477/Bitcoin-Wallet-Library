//! Playground for the Phase 1 API. Run with: `cargo run --example playground`

use wallet_library::keys::{self, WordCount};
use wallet_library::{KeychainKind, Network, Wallet, WalletError};

fn main() -> Result<(), WalletError> {
    // ── 1. Create a wallet from a NEW mnemonic ──────────────────────────
    let mnemonic = keys::generate_mnemonic(WordCount::Words12)?; // or Words24
    println!("Your 12 words: {mnemonic}"); // in a real app: show once, never log
    let wallet = Wallet::from_mnemonic(&mnemonic.to_string(), None, Network::Testnet)?;

    // ── 2. Restore a wallet from an EXISTING mnemonic ───────────────────
    let phrase = "abandon abandon abandon abandon abandon abandon \
                  abandon abandon abandon abandon abandon about";
    let mut restored = Wallet::from_mnemonic(phrase, None, Network::Bitcoin)?;
    // Optional BIP39 passphrase ("25th word") → a completely different wallet:
    let _hidden = Wallet::from_mnemonic(phrase, Some("my secret"), Network::Bitcoin)?;

    // ── 3. Generate receive addresses ───────────────────────────────────
    let addr = restored.new_address()?;
    println!("receive #{} → {}", addr.index, addr.address); // #0 → bc1qcr8te4...
    let addr = restored.new_address()?;
    println!("receive #{} → {}", addr.index, addr.address); // #1, a new address

    // ── 4. Generate change addresses (a separate counter) ───────────────
    let change = restored.new_change_address()?;
    println!("change  #{} → {}", change.index, change.address);

    // ── 5. Inspect wallet identity/state ────────────────────────────────
    println!("network: {}", restored.network());
    println!("next receive index: {}", restored.next_derivation_index(KeychainKind::External));
    println!("next change index:  {}", restored.next_derivation_index(KeychainKind::Internal));

    // ── 6. Create a wallet from a DESCRIPTOR (watch-only) ───────────────
    let external = wallet.public_descriptor(KeychainKind::External); // safe to share
    let internal = wallet.public_descriptor(KeychainKind::Internal);
    println!("descriptor: {external}");
    let mut watch_only = Wallet::from_descriptor(&external, Some(&internal), Network::Testnet)?;
    println!("watch-only #0 → {}", watch_only.new_address()?); // same as wallet's #0
    // Receive-only wallet: a single descriptor, no separate change descriptor
    let _single = Wallet::from_descriptor(&external, None, Network::Testnet)?;

    // ── 7. Handle errors ────────────────────────────────────────────────
    match Wallet::from_mnemonic("not a real mnemonic", None, Network::Bitcoin) {
        Err(WalletError::InvalidMnemonic(msg)) => println!("bad mnemonic: {msg}"),
        Err(e) => println!("other error: {e}"),
        Ok(_) => unreachable!(),
    }
    if let Err(e) = Wallet::from_descriptor(&external, None, Network::Bitcoin) {
        println!("rejected: {e}"); // network mismatch: testnet keys on mainnet
    }

    Ok(())
}
