use aes_gcm_legacy::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use octofhir_config::SecretValue;

#[test]
fn stored_secrets_interoperate_with_aes_gcm_010() {
    let values = ["", "API secret", "Секрет — 日本語 🔐", "a\nwith\tcontrols"];
    for byte in [0u8, 1, 127, 255] {
        let key = [byte; 32];
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        for (index, value) in values.iter().enumerate() {
            let mut nonce = [byte; 12];
            nonce[0] = index as u8;
            let ciphertext = cipher
                .encrypt(Nonce::from_slice(&nonce), value.as_bytes())
                .unwrap();
            let stored = SecretValue {
                encrypted: true,
                ciphertext: STANDARD.encode(ciphertext),
                nonce: STANDARD.encode(nonce),
                key_id: "legacy".into(),
            };
            assert_eq!(stored.decrypt(&key).unwrap(), *value);
            let current = SecretValue::encrypt(value, &key, "current").unwrap();
            let nonce = STANDARD.decode(&current.nonce).unwrap();
            let ciphertext = STANDARD.decode(&current.ciphertext).unwrap();
            let plaintext = cipher
                .decrypt(Nonce::from_slice(&nonce), ciphertext.as_slice())
                .unwrap();
            assert_eq!(plaintext, value.as_bytes());
            let mut tampered = current;
            let mut bytes = STANDARD.decode(&tampered.ciphertext).unwrap();
            bytes[0] ^= 1;
            tampered.ciphertext = STANDARD.encode(bytes);
            assert!(tampered.decrypt(&key).is_err());
        }
    }
}
