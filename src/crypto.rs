#[cfg(test)]
mod tests;

use std::fmt::Display;

use sodiumoxide::crypto::{sealedbox, secretbox, box_};

#[derive(Clone)]
pub(crate) struct SecretBox {
    key: secretbox::Key,
}

impl SecretBox {
    pub(crate) fn generate() -> Self {
        Self {
            key: secretbox::gen_key()
        }
    }

    pub(crate) fn encrypt(&self, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(secretbox::NONCEBYTES + data.len());
        let nonce = secretbox::gen_nonce();
        let cypher = secretbox::seal(data, &nonce, &self.key);
        out.extend_from_slice(nonce.as_ref());
        out.extend_from_slice(&cypher);
        out
    }

    pub(crate) fn decrypt(&self, data: &[u8]) -> anyhow::Result<Vec<u8>> {
        if secretbox::NONCEBYTES > data.len() {
            return Err(anyhow::format_err!("Expected at least {} bytes for the nonce", secretbox::NONCEBYTES));
        }
        let (nonce_bytes, cypher) = data.split_at(secretbox::NONCEBYTES);
        let nonce = secretbox::Nonce::from_slice(nonce_bytes).expect("We specified the right nonce size.");

        secretbox::open(cypher, &nonce, &self.key).map_err(
            |_| anyhow::format_err!("Error decrypting.")
        )
    }
}

#[derive(Clone, PartialEq)]
pub(crate) struct SealedBoxPublicKey {
    key: box_::PublicKey,
}

impl SealedBoxPublicKey {
    pub fn from_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
        match box_::PublicKey::from_slice(bytes) {
            Some(key) => Ok(Self{key}),
            None => Err(anyhow::format_err!("Wrong number of public key bytes")),
        }
    }

    pub fn from_base58(value: &str) -> anyhow::Result<Self> {
        let bytes = bs58::decode(value).into_vec()?;
        Self::from_bytes(&bytes)
    }

    pub fn encrypt(&self, bytes: &[u8]) -> Vec<u8> {
        sealedbox::seal(bytes, &self.key)
    }
}



impl Display for SealedBoxPublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", bs58::encode(self.key.as_ref()).into_string())
    }
}

#[derive(Clone)]
pub(crate) struct SealedBoxPrivateKey {
    public_key: SealedBoxPublicKey,
    private_key: box_::SecretKey
}

impl SealedBoxPrivateKey {
    pub fn generate() -> Self {
        let (pub_key, priv_key) = box_::gen_keypair();
        Self{
            public_key: SealedBoxPublicKey{key: pub_key},
            private_key: priv_key
        }
    }

    pub fn from_base58(value: &str) -> anyhow::Result<Self> {
        let bytes = bs58::decode(value).into_vec()?;
        Self::from_bytes(&bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
        let private_key = match box_::SecretKey::from_slice(bytes) {
            Some(key) => key,
            None => return Err(anyhow::format_err!("Wrong number of private key bytes")),
        };

        Ok(Self {
            public_key: SealedBoxPublicKey{key: private_key.public_key()},
            private_key,
        })
    }

    // Oops, the Deno version of Vault used to give out the seed. Can try this.
    pub fn from_base58_seed(value: &str) -> anyhow::Result<Self> {
        let bytes = bs58::decode(value).into_vec()?;
        let seed = box_::Seed::from_slice(&bytes).ok_or_else(|| anyhow::format_err!("Wrong number of seed bytes"))?;
        let (public_key, private_key) = box_::keypair_from_seed(&seed);
        Ok(Self{
            private_key,
            public_key: SealedBoxPublicKey {key: public_key},
        })
    }

    pub fn public(&self) -> &SealedBoxPublicKey { &self.public_key }

    pub fn decrypt(&self, bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
        sealedbox::open(bytes, &self.public_key.key, &self.private_key).map_err(
            |_| anyhow::format_err!("Error decrypting")
        )
    }

    pub fn decrypt_string(&self, bytes: &[u8]) -> anyhow::Result<String> {
        let decrypted = self.decrypt(bytes)?;
        Ok(String::from_utf8(decrypted)?)
    }

    pub fn bytes(&self) -> &[u8] {
        return &self.private_key.as_ref();
    }

}

impl Display for SealedBoxPrivateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", bs58::encode(self.private_key.as_ref()).into_string())
    }
}