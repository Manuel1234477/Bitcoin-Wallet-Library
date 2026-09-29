use wallet_library::keys::{self, WordCount};
use wallet_library::{KeychainKind, Network, Wallet, WalletError};

// Official BIP84 test vector:
// https://github.com/bitcoin/bips/blob/master/bip-0084.mediawiki#test-vectors
const MNEMONIC: &str = "abandon abandon abandon abandon abandon abandon \
                        abandon abandon abandon abandon abandon about";

#[test]
fn mnemonic_wallet_matches_bip84_vectors() {
    let mut wallet = Wallet::from_mnemonic(MNEMONIC, None, Network::Bitcoin).unwrap();

    let first = wallet.new_address().unwrap();
    assert_eq!(first.index, 0);
    assert_eq!(first.keychain, KeychainKind::External);
    assert_eq!(first.to_string(), "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu");
    assert_eq!(
        wallet.new_address().unwrap().to_string(),
        "bc1qnjg0jd8228aq7egyzacy8cys3knf9xvrerkf9g"
    );

    let change = wallet.new_change_address().unwrap();
    assert_eq!(change.index, 0);
    assert_eq!(change.to_string(), "bc1q8c6fshw2dlwun7ekn9qwf37cu2rn755upcp6el");
}

#[test]
fn new_address_advances_derivation_index() {
    let mut wallet = Wallet::from_mnemonic(MNEMONIC, None, Network::Testnet).unwrap();
    assert_eq!(wallet.next_derivation_index(KeychainKind::External), 0);

    let addrs: Vec<_> = (0..5).map(|_| wallet.new_address().unwrap()).collect();
    for (i, a) in addrs.iter().enumerate() {
        assert_eq!(a.index, i as u32);
        assert!(a.to_string().starts_with("tb1q"));
    }
    let unique: std::collections::HashSet<_> = addrs.iter().map(|a| a.to_string()).collect();
    assert_eq!(unique.len(), 5);
    assert_eq!(wallet.next_derivation_index(KeychainKind::External), 5);
    // Receive and change indices are tracked independently.
    assert_eq!(wallet.next_derivation_index(KeychainKind::Internal), 0);
}

#[test]
fn passphrase_changes_wallet() {
    let mut plain = Wallet::from_mnemonic(MNEMONIC, None, Network::Bitcoin).unwrap();
    let mut salted = Wallet::from_mnemonic(MNEMONIC, Some("TREZOR"), Network::Bitcoin).unwrap();
    assert_ne!(plain.new_address().unwrap(), salted.new_address().unwrap());
}

#[test]
fn watch_only_descriptor_wallet_reproduces_addresses() {
    let mut full = Wallet::from_mnemonic(MNEMONIC, None, Network::Testnet).unwrap();
    let external = full.public_descriptor(KeychainKind::External);
    let internal = full.public_descriptor(KeychainKind::Internal);
    assert!(external.contains("tpub"), "public descriptor must not contain private keys");

    let mut watch = Wallet::from_descriptor(&external, Some(&internal), Network::Testnet).unwrap();
    for _ in 0..3 {
        assert_eq!(full.new_address().unwrap(), watch.new_address().unwrap());
    }
    assert_eq!(full.new_change_address().unwrap(), watch.new_change_address().unwrap());
}

#[test]
fn single_descriptor_wallet() {
    let full = Wallet::from_mnemonic(MNEMONIC, None, Network::Testnet).unwrap();
    let external = full.public_descriptor(KeychainKind::External);
    let mut wallet = Wallet::from_descriptor(&external, None, Network::Testnet).unwrap();
    assert_eq!(wallet.new_address().unwrap().index, 0);
}

#[test]
fn generated_mnemonic_creates_wallet() {
    let mnemonic = keys::generate_mnemonic(WordCount::Words12).unwrap();
    assert_eq!(mnemonic.word_count(), 12);
    let mut wallet =
        Wallet::from_mnemonic(&mnemonic.to_string(), None, Network::Regtest).unwrap();
    assert!(wallet.new_address().unwrap().to_string().starts_with("bcrt1q"));
}

#[test]
fn rejects_bad_mnemonic() {
    let bad_word = MNEMONIC.replace("about", "zzzzz");
    assert!(matches!(
        Wallet::from_mnemonic(&bad_word, None, Network::Bitcoin),
        Err(WalletError::InvalidMnemonic(_))
    ));
    // Valid words, wrong checksum.
    let bad_checksum = MNEMONIC.replace("about", "abandon");
    assert!(matches!(
        Wallet::from_mnemonic(&bad_checksum, None, Network::Bitcoin),
        Err(WalletError::InvalidMnemonic(_))
    ));
}

#[test]
fn rejects_malformed_descriptor() {
    assert!(matches!(
        Wallet::from_descriptor("wpkh(not-a-key)", None, Network::Bitcoin),
        Err(WalletError::InvalidDescriptor(_))
    ));
}

#[test]
fn rejects_descriptor_for_wrong_network() {
    let testnet = Wallet::from_mnemonic(MNEMONIC, None, Network::Testnet).unwrap();
    let tpub_desc = testnet.public_descriptor(KeychainKind::External);
    assert!(matches!(
        Wallet::from_descriptor(&tpub_desc, None, Network::Bitcoin),
        Err(WalletError::NetworkMismatch(_))
    ));
}

#[test]
fn rejects_descriptor_without_wildcard() {
    let single_key = "wpkh(0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798)";
    assert!(matches!(
        Wallet::from_descriptor(single_key, None, Network::Bitcoin),
        Err(WalletError::InvalidDescriptor(_))
    ));
}

#[test]
fn debug_output_hides_secrets() {
    let wallet = Wallet::from_mnemonic(MNEMONIC, None, Network::Bitcoin).unwrap();
    let debug = format!("{wallet:?}");
    assert!(!debug.contains("xprv") && !debug.contains("abandon"), "{debug}");
}
