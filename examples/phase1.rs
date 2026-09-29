//! Phase 1 walkthrough: create a wallet, generate addresses, inspect state.
//!
//! Run with: `cargo run --example phase1`

use wallet_library::keys::{self, WordCount};
use wallet_library::{KeychainKind, Network, Wallet};

fn main() -> wallet_library::Result<()> {
    // Path 1: brand-new mnemonic → descriptor → wallet.
    let mnemonic = keys::generate_mnemonic(WordCount::Words12)?;
    println!("New mnemonic (demo only, never print real ones): {mnemonic}");
    let mut wallet = Wallet::from_mnemonic(&mnemonic.to_string(), None, Network::Testnet)?;

    for _ in 0..3 {
        let addr = wallet.new_address()?;
        println!("receive #{}: {}", addr.index, addr);
    }
    let change = wallet.new_change_address()?;
    println!("change  #{}: {}", change.index, change);
    println!(
        "next receive index: {}",
        wallet.next_derivation_index(KeychainKind::External)
    );

    // Path 2: descriptor → wallet (watch-only, from the public descriptors).
    let external = wallet.public_descriptor(KeychainKind::External);
    let internal = wallet.public_descriptor(KeychainKind::Internal);
    println!("\nexternal descriptor: {external}");
    let mut watch_only = Wallet::from_descriptor(&external, Some(&internal), Network::Testnet)?;
    println!("watch-only receive #0: {}", watch_only.new_address()?);

    Ok(())
}
