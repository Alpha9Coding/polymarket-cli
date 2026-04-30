//! Race-test runner for measuring CLOB submit/cancel timing.
//!
//! Single-process, single-tokio-runtime. All orders are built + signed +
//! their EIP-712 hashes (the order_ids) computed BEFORE the measurement
//! starts, so cancel can fire by-id even if the prior submit's response
//! has not arrived yet.
//!
//! Multi-signer support (v0.5.0+): Polymarket forbids self-matching, so
//! tests where a maker and a taker must collide need two distinct wallets.
//! Pass repeated `--signer LABEL=KEY[:SIG_TYPE]` and tag each `--order`
//! with `@SIGNER_LABEL`. Each signer is L1+L2 authenticated once during
//! setup; submits and cancels for an order use that order's signer's
//! authenticated client.

use std::borrow::Cow;
use std::collections::HashMap;
use std::str::FromStr;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use alloy::dyn_abi::Eip712Domain;
use alloy::primitives::U256 as AlloyU256;
use alloy::signers::local::PrivateKeySigner;
use alloy::sol_types::SolStruct as _;
use anyhow::{Context, Result, anyhow, bail};
use clap::Args;
use polymarket_client_sdk_v2::auth::Normal;
use polymarket_client_sdk_v2::auth::Signer as _;
use polymarket_client_sdk_v2::auth::state::Authenticated;
use polymarket_client_sdk_v2::clob::types::SignatureType;
use polymarket_client_sdk_v2::clob::types::response::{CancelOrdersResponse, PostOrderResponse};
use polymarket_client_sdk_v2::clob::types::{OrderPayload, OrderType, Side, SignedOrder};
use polymarket_client_sdk_v2::{POLYGON, clob, contract_config};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use tokio::task::JoinHandle;

use crate::auth;
use crate::commands::clob::{CliOrderType, CliSide, parse_token_id};
use crate::output::OutputFormat;

/// Sentinel signer label used when callers pass a single `--private-key`
/// (or rely on the config wallet) and orders don't tag a `@SIGNER`.
const DEFAULT_SIGNER_LABEL: &str = "<default>";

#[derive(Args)]
pub struct RaceArgs {
    /// Signer definition: `LABEL=PRIVATE_KEY[:SIG_TYPE]` (repeatable).
    /// SIG_TYPE is `proxy` (default), `eoa`, or `gnosis-safe`.
    /// When this flag is used, every `--order` MUST tag a signer via `@LABEL`.
    /// When omitted, the global `--private-key` (or config wallet) is used and
    /// orders should NOT include `@SIGNER`.
    #[arg(long = "signer", value_parser = parse_signer_spec, action = clap::ArgAction::Append)]
    pub signers: Vec<SignerSpec>,

    /// Order definition: `LABEL=TOKEN:SIDE:PRICE:SIZE[:TYPE][@SIGNER]` (repeat for each order).
    /// TOKEN is a hex `0x…` (or large decimal) CLOB token id. SIDE is buy|sell.
    /// TYPE is GTC (default) | FOK | GTD | FAK.
    /// SIGNER (optional) references a `--signer` label; if `--signer` is used at all,
    /// every `--order` must include `@SIGNER`.
    #[arg(long = "order", value_parser = parse_order_spec, action = clap::ArgAction::Append)]
    pub orders: Vec<OrderSpec>,

    /// Action: `submit:LABEL` | `cancel:LABEL` | `wait:MS` (repeat in order).
    #[arg(long = "step", value_parser = parse_step_spec, action = clap::ArgAction::Append)]
    pub steps: Vec<StepSpec>,

    /// Run the whole plan N times (each run uses fresh salts/order_ids).
    #[arg(long, default_value = "1")]
    pub repeat: u32,

    /// Pre-warm the HTTP connection pool with a cheap unauthenticated call before timing starts.
    #[arg(long)]
    pub warmup: bool,

    /// Build + sign + compute order_ids without submitting (validate the plan + check balances).
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Clone, Debug)]
pub struct SignerSpec {
    pub label: String,
    pub private_key: String,
    pub signature_type: Option<String>,
}

fn parse_signer_spec(s: &str) -> std::result::Result<SignerSpec, String> {
    let (label, rest) = s
        .split_once('=')
        .ok_or_else(|| format!("expected `LABEL=PRIVATE_KEY[:SIG_TYPE]`, got `{s}`"))?;
    if label.is_empty() {
        return Err("--signer label must be non-empty".into());
    }
    if label == DEFAULT_SIGNER_LABEL {
        return Err(format!(
            "`{DEFAULT_SIGNER_LABEL}` is reserved as a sentinel"
        ));
    }
    // PRIVATE_KEY may itself contain a single `:` if the user passes `:SIG_TYPE`.
    // Parse from the right so the key (a hex string with no `:`) stays intact.
    let (private_key, sig_type) = match rest.rsplit_once(':') {
        // `0xKEY:TYPE` — only treat the suffix as a type if it's a known label;
        // otherwise it's part of an unusual key (shouldn't happen for hex keys).
        Some((k, t)) if matches!(t.to_lowercase().as_str(), "proxy" | "eoa" | "gnosis-safe") => {
            (k.to_string(), Some(t.to_string()))
        }
        _ => (rest.to_string(), None),
    };
    Ok(SignerSpec {
        label: label.to_string(),
        private_key,
        signature_type: sig_type,
    })
}

#[derive(Clone, Debug)]
pub struct OrderSpec {
    pub label: String,
    pub token: String,
    pub side: CliSide,
    pub price: String,
    pub size: String,
    pub order_type: CliOrderType,
    /// `Some(label)` if the user wrote `@LABEL`, otherwise `None` (use default).
    pub signer: Option<String>,
}

fn parse_order_spec(s: &str) -> std::result::Result<OrderSpec, String> {
    let (label, rest) = s.split_once('=').ok_or_else(|| {
        format!("expected `LABEL=TOKEN:SIDE:PRICE:SIZE[:TYPE][@SIGNER]`, got `{s}`")
    })?;
    let (rest, signer) = match rest.rsplit_once('@') {
        Some((before, sig)) if !sig.is_empty() => (before, Some(sig.to_string())),
        _ => (rest, None),
    };
    let parts: Vec<&str> = rest.split(':').collect();
    if parts.len() < 4 || parts.len() > 5 {
        return Err(format!(
            "expected `LABEL=TOKEN:SIDE:PRICE:SIZE[:TYPE][@SIGNER]` (4 or 5 colon-separated fields), got `{s}`"
        ));
    }
    let token = parts[0].to_string();
    let side = match parts[1].to_lowercase().as_str() {
        "buy" => CliSide::Buy,
        "sell" => CliSide::Sell,
        other => return Err(format!("unknown side `{other}`, expected buy|sell")),
    };
    let price = parts[2].to_string();
    let size = parts[3].to_string();
    let order_type = match parts
        .get(4)
        .copied()
        .unwrap_or("GTC")
        .to_uppercase()
        .as_str()
    {
        "GTC" => CliOrderType::Gtc,
        "FOK" => CliOrderType::Fok,
        "GTD" => CliOrderType::Gtd,
        "FAK" => CliOrderType::Fak,
        other => {
            return Err(format!(
                "unknown order type `{other}`, expected GTC|FOK|GTD|FAK"
            ));
        }
    };
    Ok(OrderSpec {
        label: label.to_string(),
        token,
        side,
        price,
        size,
        order_type,
        signer,
    })
}

#[derive(Clone, Debug)]
pub enum StepSpec {
    Submit(String),
    Cancel(String),
    Wait(u64),
}

fn parse_step_spec(s: &str) -> std::result::Result<StepSpec, String> {
    let (kind, arg) = s
        .split_once(':')
        .ok_or_else(|| format!("expected `KIND:ARG` (e.g. `submit:m1`, `wait:10`), got `{s}`"))?;
    Ok(match kind.to_lowercase().as_str() {
        "submit" => StepSpec::Submit(arg.to_string()),
        "cancel" => StepSpec::Cancel(arg.to_string()),
        "wait" => StepSpec::Wait(
            arg.parse::<u64>()
                .map_err(|_| format!("invalid wait ms `{arg}`"))?,
        ),
        other => {
            return Err(format!(
                "unknown step kind `{other}`, expected submit|cancel|wait"
            ));
        }
    })
}

/// Per-signer state held between setup and execution.
struct SignerBundle {
    signer: PrivateKeySigner,
    client: clob::Client<Authenticated<Normal>>,
}

pub async fn execute(
    args: RaceArgs,
    output: &OutputFormat,
    private_key: Option<&str>,
    signature_type: Option<&str>,
) -> Result<()> {
    if args.orders.is_empty() {
        bail!("at least one --order is required");
    }
    if args.steps.is_empty() {
        bail!("at least one --step is required");
    }

    // ----- Validate plan structure -----

    // Order labels unique.
    let mut seen_orders = HashMap::new();
    for o in &args.orders {
        if seen_orders.insert(o.label.clone(), ()).is_some() {
            bail!("duplicate --order label `{}`", o.label);
        }
    }
    // Step references resolve.
    for step in &args.steps {
        match step {
            StepSpec::Submit(l) | StepSpec::Cancel(l) => {
                if !seen_orders.contains_key(l) {
                    bail!("step references unknown order label `{l}`");
                }
            }
            StepSpec::Wait(_) => {}
        }
    }
    // Signer labels unique.
    let mut seen_signers = HashMap::new();
    for s in &args.signers {
        if seen_signers.insert(s.label.clone(), ()).is_some() {
            bail!("duplicate --signer label `{}`", s.label);
        }
    }
    // If --signer is used at all, every order must tag @SIGNER and the labels must resolve.
    let multi_signer_mode = !args.signers.is_empty();
    for o in &args.orders {
        match (&o.signer, multi_signer_mode) {
            (None, true) => bail!(
                "--signer was provided, so every --order must end with `@SIGNER` — \
                 order `{}` is missing it",
                o.label
            ),
            (Some(sig), true) => {
                if !seen_signers.contains_key(sig) {
                    bail!(
                        "--order {} references unknown signer `{sig}`; \
                         define it with `--signer {sig}=0xKEY[:SIG_TYPE]`",
                        o.label
                    );
                }
            }
            (Some(_), false) => bail!(
                "--order {} uses `@SIGNER` but no --signer was defined; \
                 either drop the `@SIGNER` suffix or add `--signer LABEL=0xKEY`",
                o.label
            ),
            (None, false) => {}
        }
    }

    // ----- Build per-signer authenticated bundles -----

    let mut bundles: HashMap<String, SignerBundle> = HashMap::new();
    if multi_signer_mode {
        for spec in &args.signers {
            let bundle = build_bundle(&spec.private_key, spec.signature_type.as_deref())
                .await
                .with_context(|| format!("authenticate signer `{}` failed", spec.label))?;
            bundles.insert(spec.label.clone(), bundle);
        }
    } else {
        // Backward-compat: single default signer from --private-key flag or config.
        let (key, _) = crate::config::resolve_key(private_key)?;
        let key = key.ok_or_else(|| anyhow!("{}", crate::config::NO_WALLET_MSG))?;
        let bundle = build_bundle(&key, signature_type)
            .await
            .context("authenticate default signer failed")?;
        bundles.insert(DEFAULT_SIGNER_LABEL.to_string(), bundle);
    }

    if args.warmup {
        // Warm the connection pool by hitting the unauthenticated `ok` endpoint
        // through one of the bundles' clients (the host is the same regardless).
        let any_client = &bundles.values().next().unwrap().client;
        let _ = any_client.ok().await;
    }

    // ----- Run -----

    let mut runs = Vec::with_capacity(args.repeat as usize);
    for run_idx in 0..args.repeat {
        let run = run_one(&args, &bundles, multi_signer_mode, run_idx).await?;
        runs.push(run);
    }

    let payload = if runs.len() == 1 {
        runs.into_iter().next().unwrap()
    } else {
        json!({ "runs": runs })
    };
    match output {
        OutputFormat::Json | OutputFormat::Table => {
            // Race output is structured; table mode also prints JSON for fidelity.
            println!("{}", serde_json::to_string_pretty(&payload)?);
        }
    }
    Ok(())
}

/// Authenticate one private key end-to-end and return the bundle. Mirrors
/// `auth::authenticate_with_signer` but exposes the concrete signer so we
/// can still `.sign()` orders later.
async fn build_bundle(private_key: &str, sig_type_flag: Option<&str>) -> Result<SignerBundle> {
    let signer = PrivateKeySigner::from_str(private_key)
        .context("Invalid private key")?
        .with_chain_id(Some(POLYGON));
    let resolved_sig_type = crate::config::resolve_signature_type(sig_type_flag)?;
    let sig_type = parse_signature_type(&resolved_sig_type);
    let client = auth::unauthenticated_clob_client()?
        .authentication_builder(&signer)
        .signature_type(sig_type)
        .authenticate()
        .await
        .context("Failed to authenticate with Polymarket CLOB")?;
    Ok(SignerBundle { signer, client })
}

fn parse_signature_type(s: &str) -> SignatureType {
    match s {
        "proxy" => SignatureType::Proxy,
        "gnosis-safe" => SignatureType::GnosisSafe,
        _ => SignatureType::Eoa,
    }
}

async fn run_one(
    args: &RaceArgs,
    bundles: &HashMap<String, SignerBundle>,
    multi_signer_mode: bool,
    run_idx: u32,
) -> Result<Value> {
    // ---------- setup phase (NOT timed) ----------
    let mut signed_by_label: HashMap<String, SignedOrder> = HashMap::new();
    let mut order_id_by_label: HashMap<String, String> = HashMap::new();
    let mut signer_by_label: HashMap<String, String> = HashMap::new(); // order label → signer label
    let mut order_meta: serde_json::Map<String, Value> = serde_json::Map::new();

    for spec in &args.orders {
        let signer_label = if multi_signer_mode {
            spec.signer.clone().unwrap()
        } else {
            DEFAULT_SIGNER_LABEL.to_string()
        };
        let bundle = bundles
            .get(&signer_label)
            .ok_or_else(|| anyhow!("internal: no bundle for signer `{signer_label}`"))?;

        let token_id = parse_token_id(&spec.token)
            .with_context(|| format!("--order {} bad token `{}`", spec.label, spec.token))?;
        let price_dec = Decimal::from_str(&spec.price)
            .with_context(|| format!("--order {} bad price `{}`", spec.label, spec.price))?;
        let size_dec = Decimal::from_str(&spec.size)
            .with_context(|| format!("--order {} bad size `{}`", spec.label, spec.size))?;

        let signable = bundle
            .client
            .limit_order()
            .token_id(token_id)
            .side(Side::from(spec.side))
            .price(price_dec)
            .size(size_dec)
            .order_type(OrderType::from(spec.order_type))
            .build()
            .await
            .with_context(|| format!("build {} failed", spec.label))?;

        let signed = bundle
            .client
            .sign(&bundle.signer, signable)
            .await
            .with_context(|| format!("sign {} failed", spec.label))?;

        let order_id = compute_order_id(&bundle.client, &signed, token_id).await?;

        order_meta.insert(
            spec.label.clone(),
            json!({
                "order_id": order_id,
                "token_id": spec.token,
                "side": format!("{:?}", spec.side),
                "price": spec.price,
                "size": spec.size,
                "order_type": format!("{:?}", spec.order_type),
                "signer": signer_label,
                "maker": format!("{}", bundle.signer.address()),
            }),
        );
        order_id_by_label.insert(spec.label.clone(), order_id);
        signer_by_label.insert(spec.label.clone(), signer_label);
        signed_by_label.insert(spec.label.clone(), signed);
    }

    if args.dry_run {
        return Ok(json!({
            "run": run_idx,
            "dry_run": true,
            "multi_signer": multi_signer_mode,
            "signers": bundles.keys().cloned().collect::<Vec<_>>(),
            "orders": Value::Object(order_meta),
            "steps": args.steps.iter().map(step_summary).collect::<Vec<_>>(),
        }));
    }

    // ---------- execution phase (timed) ----------
    let t0_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let t0 = Instant::now();

    type TaskHandle = JoinHandle<(Instant, std::result::Result<Value, String>)>;
    let mut inflight: Vec<(usize, TaskHandle)> = Vec::new();
    let mut records: Vec<Value> = Vec::with_capacity(args.steps.len());

    for (i, step) in args.steps.iter().enumerate() {
        match step {
            StepSpec::Submit(label) => {
                let signed = signed_by_label.remove(label).ok_or_else(|| {
                    anyhow!("internal: order `{label}` already consumed by an earlier submit")
                })?;
                let signer_label = signer_by_label
                    .get(label)
                    .cloned()
                    .unwrap_or_else(|| DEFAULT_SIGNER_LABEL.to_string());
                let bundle = bundles.get(&signer_label).ok_or_else(|| {
                    anyhow!("internal: missing bundle for signer `{signer_label}`")
                })?;
                let client = bundle.client.clone();
                let t_send_us = t0.elapsed().as_micros();
                let handle: TaskHandle = tokio::spawn(async move {
                    let result = client.post_order(signed).await;
                    let t_finish = Instant::now();
                    let value = match result {
                        Ok(resp) => Ok(post_order_response_to_json(&resp)),
                        Err(e) => Err(format!("{e:#}")),
                    };
                    (t_finish, value)
                });
                inflight.push((i, handle));
                records.push(json!({
                    "step": i,
                    "kind": "submit",
                    "label": label,
                    "signer": signer_label,
                    "order_id": order_id_by_label.get(label).cloned().unwrap_or_default(),
                    "t_send_us": t_send_us,
                }));
            }
            StepSpec::Cancel(label) => {
                let order_id = order_id_by_label
                    .get(label)
                    .ok_or_else(|| anyhow!("internal: missing order_id for `{label}`"))?
                    .clone();
                let oid_for_record = order_id.clone();
                let signer_label = signer_by_label
                    .get(label)
                    .cloned()
                    .unwrap_or_else(|| DEFAULT_SIGNER_LABEL.to_string());
                let bundle = bundles.get(&signer_label).ok_or_else(|| {
                    anyhow!("internal: missing bundle for signer `{signer_label}`")
                })?;
                let client = bundle.client.clone();
                let t_send_us = t0.elapsed().as_micros();
                let handle: TaskHandle = tokio::spawn(async move {
                    let result = client.cancel_order(&order_id).await;
                    let t_finish = Instant::now();
                    let value = match result {
                        Ok(resp) => Ok(cancel_orders_response_to_json(&resp)),
                        Err(e) => Err(format!("{e:#}")),
                    };
                    (t_finish, value)
                });
                inflight.push((i, handle));
                records.push(json!({
                    "step": i,
                    "kind": "cancel",
                    "label": label,
                    "signer": signer_label,
                    "order_id": oid_for_record,
                    "t_send_us": t_send_us,
                }));
            }
            StepSpec::Wait(ms) => {
                records.push(json!({
                    "step": i,
                    "kind": "wait",
                    "duration_ms": ms,
                }));
                tokio::time::sleep(Duration::from_millis(*ms)).await;
            }
        }
    }

    // ---------- collect responses ----------
    for (idx, handle) in inflight {
        let (t_finish, value) = handle.await.map_err(|e| anyhow!("join error: {e}"))?;
        let t_recv_us = t_finish.duration_since(t0).as_micros();
        let rec = records[idx].as_object_mut().expect("record is object");
        rec.insert("t_recv_us".into(), json!(t_recv_us));
        match value {
            Ok(v) => {
                rec.insert("ok".into(), json!(true));
                rec.insert("response".into(), v);
            }
            Err(e) => {
                rec.insert("ok".into(), json!(false));
                rec.insert("error".into(), json!(e));
            }
        }
    }

    Ok(json!({
        "run": run_idx,
        "t0_unix_ms": t0_unix_ms,
        "multi_signer": multi_signer_mode,
        "orders": Value::Object(order_meta),
        "actions": records,
    }))
}

fn post_order_response_to_json(resp: &PostOrderResponse) -> Value {
    json!({
        "order_id": resp.order_id,
        "status": format!("{:?}", resp.status),
        "success": resp.success,
        "making_amount": resp.making_amount.to_string(),
        "taking_amount": resp.taking_amount.to_string(),
        "error_msg": resp.error_msg,
        "transaction_hashes": resp.transaction_hashes.iter().map(|h| format!("{h:#x}")).collect::<Vec<_>>(),
        "trade_ids": resp.trade_ids,
    })
}

fn cancel_orders_response_to_json(resp: &CancelOrdersResponse) -> Value {
    json!({
        "canceled": resp.canceled,
        "not_canceled": resp.not_canceled,
    })
}

fn step_summary(s: &StepSpec) -> Value {
    match s {
        StepSpec::Submit(l) => json!({"kind": "submit", "label": l}),
        StepSpec::Cancel(l) => json!({"kind": "cancel", "label": l}),
        StepSpec::Wait(ms) => json!({"kind": "wait", "duration_ms": ms}),
    }
}

/// Replicate the SDK's domain construction in `sign()` to derive the
/// EIP-712 typed-data hash locally — this hash IS the order_id used by
/// the cancel endpoint and the on-chain settlement contract.
async fn compute_order_id(
    client: &clob::Client<Authenticated<Normal>>,
    signed: &SignedOrder,
    token_id: AlloyU256,
) -> Result<String> {
    let neg_risk = client.neg_risk(token_id).await?.neg_risk;
    let config = contract_config(POLYGON, neg_risk)
        .ok_or_else(|| anyhow!("no contract config for chain_id={POLYGON}, neg_risk={neg_risk}"))?;

    let chain_id = AlloyU256::from(POLYGON);
    let hash = match &signed.payload {
        OrderPayload::V2(p) => {
            let exchange_v2 = config.exchange_v2.ok_or_else(|| {
                anyhow!("no V2 exchange configured for chain_id={POLYGON}, neg_risk={neg_risk}")
            })?;
            let domain = Eip712Domain {
                name: Some(Cow::Borrowed("Polymarket CTF Exchange")),
                version: Some(Cow::Borrowed("2")),
                chain_id: Some(chain_id),
                verifying_contract: Some(exchange_v2),
                salt: None,
            };
            p.order.eip712_signing_hash(&domain)
        }
        OrderPayload::V1(p) => {
            let domain = Eip712Domain {
                name: Some(Cow::Borrowed("Polymarket CTF Exchange")),
                version: Some(Cow::Borrowed("1")),
                chain_id: Some(chain_id),
                verifying_contract: Some(config.exchange),
                salt: None,
            };
            p.order.eip712_signing_hash(&domain)
        }
        // OrderPayload is #[non_exhaustive]; keep behavior explicit if SDK adds variants.
        _ => bail!("unsupported OrderPayload variant for order_id derivation"),
    };

    Ok(format!("{hash:#x}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_order_spec_full() {
        let s = "m1=0xabc:buy:0.30:5:FAK";
        let o = parse_order_spec(s).unwrap();
        assert_eq!(o.label, "m1");
        assert_eq!(o.token, "0xabc");
        assert_eq!(o.price, "0.30");
        assert_eq!(o.size, "5");
        assert!(o.signer.is_none());
        assert_eq!(format!("{:?}", o.side), "Buy");
        assert_eq!(format!("{:?}", o.order_type), "Fak");
    }

    #[test]
    fn parse_order_spec_default_type_is_gtc() {
        let s = "m1=0xabc:sell:0.30:5";
        let o = parse_order_spec(s).unwrap();
        assert_eq!(format!("{:?}", o.order_type), "Gtc");
        assert_eq!(format!("{:?}", o.side), "Sell");
    }

    #[test]
    fn parse_order_spec_with_signer_no_type() {
        let s = "maker=0xabc:buy:0.30:5@alice";
        let o = parse_order_spec(s).unwrap();
        assert_eq!(o.signer.as_deref(), Some("alice"));
        assert_eq!(o.token, "0xabc");
        assert_eq!(format!("{:?}", o.order_type), "Gtc");
    }

    #[test]
    fn parse_order_spec_with_signer_and_type() {
        let s = "fak=0xabc:sell:0.30:5:FAK@bob";
        let o = parse_order_spec(s).unwrap();
        assert_eq!(o.signer.as_deref(), Some("bob"));
        assert_eq!(format!("{:?}", o.order_type), "Fak");
    }

    #[test]
    fn parse_order_spec_rejects_bad_side() {
        assert!(parse_order_spec("m1=0xabc:long:0.30:5").is_err());
    }

    #[test]
    fn parse_order_spec_rejects_too_few_fields() {
        assert!(parse_order_spec("m1=0xabc:buy:0.30").is_err());
    }

    #[test]
    fn parse_signer_spec_basic() {
        let s = parse_signer_spec("alice=0xabc123").unwrap();
        assert_eq!(s.label, "alice");
        assert_eq!(s.private_key, "0xabc123");
        assert!(s.signature_type.is_none());
    }

    #[test]
    fn parse_signer_spec_with_sig_type() {
        let s = parse_signer_spec("bob=0xdef456:eoa").unwrap();
        assert_eq!(s.label, "bob");
        assert_eq!(s.private_key, "0xdef456");
        assert_eq!(s.signature_type.as_deref(), Some("eoa"));
    }

    #[test]
    fn parse_signer_spec_with_proxy_type() {
        let s = parse_signer_spec("alice=0xKEY:proxy").unwrap();
        assert_eq!(s.signature_type.as_deref(), Some("proxy"));
    }

    #[test]
    fn parse_signer_spec_with_gnosis_safe() {
        let s = parse_signer_spec("safe=0xKEY:gnosis-safe").unwrap();
        assert_eq!(s.signature_type.as_deref(), Some("gnosis-safe"));
    }

    #[test]
    fn parse_signer_spec_unknown_suffix_is_part_of_key() {
        // If the trailing field after `:` isn't a known sig type, it's NOT a sig type
        // (and the user probably has a mangled key — but we don't second-guess).
        let s = parse_signer_spec("a=0xKEY:notatype").unwrap();
        assert_eq!(s.private_key, "0xKEY:notatype");
        assert!(s.signature_type.is_none());
    }

    #[test]
    fn parse_signer_spec_rejects_empty_label() {
        assert!(parse_signer_spec("=0xKEY").is_err());
    }

    #[test]
    fn parse_signer_spec_rejects_default_label_collision() {
        assert!(parse_signer_spec(&format!("{DEFAULT_SIGNER_LABEL}=0xKEY")).is_err());
    }

    #[test]
    fn parse_step_spec_works() {
        assert!(matches!(parse_step_spec("submit:m1"), Ok(StepSpec::Submit(s)) if s == "m1"));
        assert!(matches!(parse_step_spec("cancel:t1"), Ok(StepSpec::Cancel(s)) if s == "t1"));
        assert!(matches!(parse_step_spec("wait:10"), Ok(StepSpec::Wait(10))));
    }

    #[test]
    fn parse_step_spec_rejects_unknown() {
        assert!(parse_step_spec("submitt:m1").is_err());
        assert!(parse_step_spec("wait:notanumber").is_err());
    }
}
