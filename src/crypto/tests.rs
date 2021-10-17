use super::SealedBoxPrivateKey;

#[test]
fn test_derive() {
    let secret = SealedBoxPrivateKey::generate();
    let secret_str = secret.to_string();

    let secret2 = SealedBoxPrivateKey::from_base58(&secret_str).unwrap();

    assert_eq!(secret.public().to_string(), secret2.public().to_string());
}