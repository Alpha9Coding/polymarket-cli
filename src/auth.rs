use std::str::FromStr;

use alloy::providers::ProviderBuilder;
use anyhow::{Context, Result};
use polymarket_client_sdk_v2::auth::state::Authenticated;
use polymarket_client_sdk_v2::auth::{LocalSigner, Normal, Signer as _};
use polymarket_client_sdk_v2::clob::types::SignatureType;
use polymarket_client_sdk_v2::clob::{Client, Config};
use polymarket_client_sdk_v2::types::Address;
use polymarket_client_sdk_v2::{POLYGON, clob};

use crate::config;

const DEFAULT_RPC_URL: &str = "https://polygon.drpc.org";

/// Polymarket CLOB host. The SDK's default `clob-v2.polymarket.com` 301-redirects
/// every path (including POST `/auth/api-key`) to `clob.polymarket.com`; reqwest's
/// default redirect policy downgrades POST→GET on 301, which breaks api-key creation
/// (server returns 405 for GET on that path) and order submission. Hitting the canonical
/// host directly avoids the redirect entirely. Override with `POLYMARKET_CLOB_HOST` for
/// testing or future migrations.
const DEFAULT_CLOB_HOST: &str = "https://clob.polymarket.com";

fn clob_host() -> String {
    std::env::var("POLYMARKET_CLOB_HOST").unwrap_or_else(|_| DEFAULT_CLOB_HOST.to_string())
}

pub fn unauthenticated_clob_client() -> Result<Client> {
    Client::new(&clob_host(), Config::default())
        .context("Failed to construct CLOB client (bad POLYMARKET_CLOB_HOST?)")
}

fn rpc_url() -> String {
    std::env::var("POLYMARKET_RPC_URL").unwrap_or_else(|_| DEFAULT_RPC_URL.to_string())
}

fn parse_signature_type(s: &str) -> SignatureType {
    match s {
        config::DEFAULT_SIGNATURE_TYPE => SignatureType::Proxy,
        "gnosis-safe" => SignatureType::GnosisSafe,
        _ => SignatureType::Eoa,
    }
}

/// Resolve the optional funder (order `maker`) address. Priority: flag > env > config.
pub fn resolve_funder_address(funder_flag: Option<&str>) -> Result<Option<Address>> {
    match config::resolve_funder(funder_flag)? {
        None => Ok(None),
        Some(s) => {
            let addr = Address::from_str(s.trim())
                .with_context(|| format!("Invalid funder address `{s}` (expected 0x + 40 hex)"))?;
            anyhow::ensure!(
                addr != Address::ZERO,
                "Funder address must not be the zero address"
            );
            Ok(Some(addr))
        }
    }
}

/// Decide the effective [`SignatureType`] given the user's signature-type flag and
/// whether a funder is present.
///
/// When a funder is set, the order `maker` is the funder (a Safe/proxy) and the
/// `signer` is the configured EOA — which requires a non-EOA signature type. So if
/// the user did not EXPLICITLY pick a signature type (via `--signature-type` or
/// `POLYMARKET_SIGNATURE_TYPE`), we promote to `gnosis-safe`. An explicit choice is
/// honored, except `eoa` + funder is rejected up front with a clear message (the SDK
/// would otherwise reject it deeper in the stack).
pub fn effective_signature_type(
    signature_type_flag: Option<&str>,
    has_funder: bool,
) -> Result<SignatureType> {
    if has_funder && !config::signature_type_explicitly_set(signature_type_flag) {
        return Ok(SignatureType::GnosisSafe);
    }
    let sig = parse_signature_type(&config::resolve_signature_type(signature_type_flag)?);
    anyhow::ensure!(
        !(has_funder && matches!(sig, SignatureType::Eoa)),
        "--funder sets the order maker to a Safe/proxy, which is incompatible with an EOA \
         signature type. Drop --signature-type eoa (it will default to gnosis-safe) or pass \
         --signature-type gnosis-safe / proxy explicitly."
    );
    Ok(sig)
}

pub fn resolve_signer(
    private_key: Option<&str>,
) -> Result<impl polymarket_client_sdk_v2::auth::Signer> {
    let (key, _) = config::resolve_key(private_key)?;
    let key = key.ok_or_else(|| anyhow::anyhow!("{}", config::NO_WALLET_MSG))?;
    LocalSigner::from_str(&key)
        .context("Invalid private key")
        .map(|s| s.with_chain_id(Some(POLYGON)))
}

pub async fn authenticated_clob_client(
    private_key: Option<&str>,
    signature_type_flag: Option<&str>,
    funder_flag: Option<&str>,
) -> Result<clob::Client<Authenticated<Normal>>> {
    let signer = resolve_signer(private_key)?;
    authenticate_with_signer(&signer, signature_type_flag, funder_flag).await
}

pub async fn authenticate_with_signer(
    signer: &(impl polymarket_client_sdk_v2::auth::Signer + Sync),
    signature_type_flag: Option<&str>,
    funder_flag: Option<&str>,
) -> Result<clob::Client<Authenticated<Normal>>> {
    let funder = resolve_funder_address(funder_flag)?;
    let sig_type = effective_signature_type(signature_type_flag, funder.is_some())?;

    let mut builder = unauthenticated_clob_client()?
        .authentication_builder(signer)
        .signature_type(sig_type);
    if let Some(addr) = funder {
        builder = builder.funder(addr);
    }
    builder
        .authenticate()
        .await
        .context("Failed to authenticate with Polymarket CLOB")
}

pub async fn create_readonly_provider() -> Result<impl alloy::providers::Provider + Clone> {
    ProviderBuilder::new()
        .connect(&rpc_url())
        .await
        .context("Failed to connect to Polygon RPC")
}

pub async fn create_provider(
    private_key: Option<&str>,
) -> Result<impl alloy::providers::Provider + Clone> {
    let (key, _) = config::resolve_key(private_key)?;
    let key = key.ok_or_else(|| anyhow::anyhow!("{}", config::NO_WALLET_MSG))?;
    let signer = LocalSigner::from_str(&key)
        .context("Invalid private key")?
        .with_chain_id(Some(POLYGON));
    ProviderBuilder::new()
        .wallet(signer)
        .connect(&rpc_url())
        .await
        .context("Failed to connect to Polygon RPC with wallet")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_signature_type_proxy() {
        assert_eq!(parse_signature_type("proxy"), SignatureType::Proxy);
    }

    #[test]
    fn parse_signature_type_gnosis_safe() {
        assert_eq!(
            parse_signature_type("gnosis-safe"),
            SignatureType::GnosisSafe
        );
    }

    #[test]
    fn parse_signature_type_eoa() {
        assert_eq!(parse_signature_type("eoa"), SignatureType::Eoa);
    }

    #[test]
    fn parse_signature_type_unknown_defaults_to_eoa() {
        assert_eq!(parse_signature_type("unknown"), SignatureType::Eoa);
    }

    #[test]
    fn funder_with_explicit_gnosis_safe_flag_is_gnosis_safe() {
        assert_eq!(
            effective_signature_type(Some("gnosis-safe"), true).unwrap(),
            SignatureType::GnosisSafe
        );
    }

    #[test]
    fn funder_with_no_explicit_flag_promotes_to_gnosis_safe() {
        // No --signature-type flag, funder present → promote to gnosis-safe even though
        // the config default would otherwise resolve to "proxy".
        assert_eq!(
            effective_signature_type(None, true).unwrap(),
            SignatureType::GnosisSafe
        );
    }

    #[test]
    fn funder_with_explicit_eoa_flag_is_rejected() {
        assert!(effective_signature_type(Some("eoa"), true).is_err());
    }

    #[test]
    fn funder_with_explicit_proxy_flag_is_proxy() {
        assert_eq!(
            effective_signature_type(Some("proxy"), true).unwrap(),
            SignatureType::Proxy
        );
    }

    #[test]
    fn no_funder_honors_explicit_eoa() {
        assert_eq!(
            effective_signature_type(Some("eoa"), false).unwrap(),
            SignatureType::Eoa
        );
    }

    #[test]
    fn resolve_funder_address_rejects_garbage() {
        assert!(resolve_funder_address(Some("not-an-address")).is_err());
    }

    #[test]
    fn resolve_funder_address_rejects_zero() {
        assert!(
            resolve_funder_address(Some("0x0000000000000000000000000000000000000000")).is_err()
        );
    }

    #[test]
    fn resolve_funder_address_parses_valid() {
        let a = resolve_funder_address(Some("0xF6F687D9c728a4fc5590D71e2e53b9D418E20E74"))
            .unwrap()
            .unwrap();
        assert_eq!(
            format!("{a:#x}"),
            "0xf6f687d9c728a4fc5590d71e2e53b9d418e20e74"
        );
    }

    #[test]
    fn resolve_funder_address_none_when_unset() {
        // Explicit empty flag → None (env/config not exercised here to keep test hermetic).
        assert!(resolve_funder_address(Some("")).unwrap().is_none());
    }
}
