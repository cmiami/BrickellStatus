use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct LocalSecrets {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    whatsapp_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    aisstream_api_key: Option<String>,
}

#[derive(Clone)]
pub(super) struct LocalSecretStore {
    path: PathBuf,
    lock: Arc<Mutex<()>>,
}

impl LocalSecretStore {
    pub(super) fn new(path: PathBuf) -> Self {
        Self {
            path,
            lock: Arc::new(Mutex::new(())),
        }
    }

    pub(super) async fn whatsapp_token(&self) -> Result<Option<String>, String> {
        let _guard = self.lock.lock().await;
        Ok(read_secrets(&self.path)?.whatsapp_token)
    }

    pub(super) async fn store_whatsapp_token(&self, token: String) -> Result<(), String> {
        let _guard = self.lock.lock().await;
        let mut secrets = read_secrets(&self.path)?;
        secrets.whatsapp_token = Some(token);
        write_secrets(&self.path, &secrets)
    }

    pub(super) async fn delete_whatsapp_token(&self) -> Result<(), String> {
        let _guard = self.lock.lock().await;
        let mut secrets = read_secrets(&self.path)?;
        secrets.whatsapp_token = None;
        write_secrets(&self.path, &secrets)
    }

    pub(super) async fn aisstream_key(&self) -> Result<Option<String>, String> {
        if let Ok(key) = std::env::var("AISSTREAM_API_KEY") {
            return Ok((!key.trim().is_empty()).then_some(key));
        }
        let _guard = self.lock.lock().await;
        Ok(read_secrets(&self.path)?.aisstream_api_key)
    }

    pub(super) async fn store_aisstream_key(&self, key: String) -> Result<(), String> {
        let _guard = self.lock.lock().await;
        let mut secrets = read_secrets(&self.path)?;
        secrets.aisstream_api_key = Some(key);
        write_secrets(&self.path, &secrets)
    }

    pub(super) async fn delete_aisstream_key(&self) -> Result<(), String> {
        let _guard = self.lock.lock().await;
        let mut secrets = read_secrets(&self.path)?;
        secrets.aisstream_api_key = None;
        write_secrets(&self.path, &secrets)
    }
}

fn read_secrets(path: &Path) -> Result<LocalSecrets, String> {
    match fs::read(path) {
        Ok(bytes) => decode_secrets(&bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(LocalSecrets::default()),
        Err(error) => Err(format!("Local credential file could not be read: {error}")),
    }
}

#[cfg(not(windows))]
fn decode_secrets(bytes: &[u8]) -> Result<LocalSecrets, String> {
    serde_json::from_slice(bytes)
        .map_err(|error| format!("Local credential file is invalid: {error}"))
}

#[cfg(not(windows))]
fn encode_secrets(secrets: &LocalSecrets) -> Result<Vec<u8>, String> {
    serde_json::to_vec(secrets)
        .map_err(|error| format!("Local credentials could not be encoded: {error}"))
}

/// On-disk form on Windows, where the file cannot carry Unix permissions:
/// the serialized [`LocalSecrets`] is sealed with user-scope DPAPI, so the
/// bytes decrypt only for this Windows account on this machine.
#[cfg(windows)]
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct DpapiEnvelope {
    dpapi_ciphertext: String,
}

#[cfg(windows)]
fn decode_secrets(bytes: &[u8]) -> Result<LocalSecrets, String> {
    use base64::Engine as _;
    use zeroize::Zeroize as _;
    // The envelope must be tried first: its JSON also parses as an (empty)
    // LocalSecrets. A pre-DPAPI plaintext file lands in the fallback and is
    // re-enveloped by the next write.
    if let Ok(envelope) = serde_json::from_slice::<DpapiEnvelope>(bytes) {
        let ciphertext = base64::engine::general_purpose::STANDARD
            .decode(envelope.dpapi_ciphertext)
            .map_err(|error| format!("Local credential file is invalid: {error}"))?;
        let mut plaintext = dpapi::unprotect(&ciphertext)?;
        let secrets = serde_json::from_slice(&plaintext)
            .map_err(|error| format!("Local credential file is invalid: {error}"));
        plaintext.zeroize();
        return secrets;
    }
    serde_json::from_slice(bytes)
        .map_err(|error| format!("Local credential file is invalid: {error}"))
}

#[cfg(windows)]
fn encode_secrets(secrets: &LocalSecrets) -> Result<Vec<u8>, String> {
    use base64::Engine as _;
    use zeroize::Zeroize as _;
    let mut plaintext = serde_json::to_vec(secrets)
        .map_err(|error| format!("Local credentials could not be encoded: {error}"))?;
    let protected = dpapi::protect(&plaintext);
    plaintext.zeroize();
    let envelope = DpapiEnvelope {
        dpapi_ciphertext: base64::engine::general_purpose::STANDARD.encode(protected?),
    };
    serde_json::to_vec(&envelope)
        .map_err(|error| format!("Local credentials could not be encoded: {error}"))
}

fn write_secrets(path: &Path, secrets: &LocalSecrets) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Local credential directory could not be created: {error}"))?;
    }
    let bytes = encode_secrets(secrets)?;
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| format!("Local credential file could not be opened: {error}"))?;
    file.write_all(&bytes)
        .map_err(|error| format!("Local credentials could not be saved: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("Local credential permissions could not be set: {error}"))?;
    }
    Ok(())
}

/// User-scope DPAPI, prompts forbidden: encryption is keyed to the logged-in
/// Windows account with no UI or elevation, and a copied file is useless on
/// another machine or account.
#[cfg(windows)]
mod dpapi {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };

    pub(super) fn protect(plaintext: &[u8]) -> Result<Vec<u8>, String> {
        transform(plaintext, true)
            .map_err(|error| format!("Local credentials could not be encrypted: {error}"))
    }

    pub(super) fn unprotect(ciphertext: &[u8]) -> Result<Vec<u8>, String> {
        transform(ciphertext, false)
            .map_err(|error| format!("Local credentials could not be decrypted: {error}"))
    }

    fn transform(input: &[u8], encrypt: bool) -> Result<Vec<u8>, std::io::Error> {
        let input_blob = CRYPT_INTEGER_BLOB {
            cbData: u32::try_from(input.len())
                .map_err(|_| std::io::Error::other("credential payload exceeds the DPAPI limit"))?,
            pbData: input.as_ptr().cast_mut(),
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        // SAFETY: input_blob borrows `input` only for the duration of the
        // call, which never writes through it; on success the API returns a
        // LocalAlloc buffer in `output`, which is copied, wiped, and released
        // before this function returns.
        let succeeded = unsafe {
            if encrypt {
                CryptProtectData(
                    &input_blob,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output,
                )
            } else {
                CryptUnprotectData(
                    &input_blob,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output,
                )
            }
        };
        if succeeded == 0 || output.pbData.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let copied =
            unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
        unsafe {
            std::ptr::write_bytes(output.pbData, 0, output.cbData as usize);
            LocalFree(output.pbData.cast());
        }
        Ok(copied)
    }
}
