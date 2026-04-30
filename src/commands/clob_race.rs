//! Race-test runner for measuring CLOB submit/cancel timing.
//!
//! Single-process, single-tokio-runtime, single-reqwest-client. All orders are
//! built + signed + their EIP-712 hashes (the order_ids) computed BEFORE the
//! measurement starts, so cancel can fire by-id even if the prior submit's
//! response has not arrived yet.

use std::borrow::Cow;
use std::collections::HashMap;
use std::str::FromStr;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use alloy::dyn_abi::Eip712Domain;
use alloy::primitives::U256 as AlloyU256;
use alloy::sol_types::SolStruct as _;
use anyhow::{Context, Result, anyhow, bail};
use clap::Args;
use polymarket_client_sdk_v2::auth::Normal;
use polymarket_client_sdk_v2::auth::state::Authenticated;
use polymarket_client_sdk_v2::clob::types::response::{CancelOrdersResponse, PostOrderResponse};
use polymarket_client_sdk_v2::clob::types::{OrderPayload, OrderType, Side, SignedOrder};
use polymarket_client_sdk_v2::{POLYGON, clob, contract_config};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use tokio::task::JoinHandle;

use crate::auth;
use crate::commands::clob::{CliOrderType, CliSide, parse_token_id};
use crate::output::OutputFormat;

#[derive(Args)]
pub struct RaceArgs {
    /// Order definition: `LABEL=TOKEN:SIDE:PRICE:SIZE[:TYPE]` (repeat for each order).
    /// TOKEN is a hex `0x…` (or large decimal) CLOB token id. SIDE is buy|sell.
    /// TYPE is GTC (default) | FOK | GTD | FAK.
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
pub struct OrderSpec {
    pub label: String,
    pub token: String,
    pub side: CliSide,
    pub price: String,
    pub size: String,
    pub order_type: CliOrderType,
}

// Side and OrderType are Copy; we keep OrderSpec by-reference and Copy these fields out per use.

fn parse_order_spec(s: &str) -> std::result::Result<OrderSpec, String> {
    let (label, rest) = s
        .split_once('=')
        .ok_or_else(|| format!("expected `LABEL=TOKEN:SIDE:PRICE:SIZE[:TYPE]`, got `{s}`"))?;
    let parts: Vec<&str> = rest.split(':').collect();
    if parts.len() < 4 || parts.len() > 5 {
        return Err(format!(
            "expected `LABEL=TOKEN:SIDE:PRICE:SIZE[:TYPE]` (4 or 5 fields after =), got `{s}`"
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

    // Validate label uniqueness + every step reference resolves.
    let mut seen = HashMap::new();
    for o in &args.orders {
        if seen.insert(o.label.clone(), ()).is_some() {
            bail!("duplicate --order label `{}`", o.label);
        }
    }
    for step in &args.steps {
        match step {
            StepSpec::Submit(l) | StepSpec::Cancel(l) => {
                if !seen.contains_key(l) {
                    bail!("step references unknown order label `{l}`");
                }
            }
            StepSpec::Wait(_) => {}
        }
    }

    let signer = auth::resolve_signer(private_key)?;
    let client = auth::authenticate_with_signer(&signer, signature_type).await?;

    if args.warmup {
        // Cheap, authenticated-but-not-state-changing call to warm the connection pool.
        // `ok()` is unauthenticated against the same host, which is what we want.
        let _ = client.ok().await;
    }

    let mut runs = Vec::with_capacity(args.repeat as usize);
    for run_idx in 0..args.repeat {
        let run = run_one(&args, &client, &signer, run_idx).await?;
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

async fn run_one(
    args: &RaceArgs,
    client: &clob::Client<Authenticated<Normal>>,
    signer: &(impl polymarket_client_sdk_v2::auth::Signer + Sync),
    run_idx: u32,
) -> Result<Value> {
    // ---------- setup phase (NOT timed) ----------
    let mut signed_by_label: HashMap<String, SignedOrder> = HashMap::new();
    let mut order_id_by_label: HashMap<String, String> = HashMap::new();
    let mut order_meta: serde_json::Map<String, Value> = serde_json::Map::new();

    for spec in &args.orders {
        let token_id = parse_token_id(&spec.token)
            .with_context(|| format!("--order {} bad token `{}`", spec.label, spec.token))?;
        let price_dec = Decimal::from_str(&spec.price)
            .with_context(|| format!("--order {} bad price `{}`", spec.label, spec.price))?;
        let size_dec = Decimal::from_str(&spec.size)
            .with_context(|| format!("--order {} bad size `{}`", spec.label, spec.size))?;

        let signable = client
            .limit_order()
            .token_id(token_id)
            .side(Side::from(spec.side))
            .price(price_dec)
            .size(size_dec)
            .order_type(OrderType::from(spec.order_type))
            .build()
            .await
            .with_context(|| format!("build {} failed", spec.label))?;

        let signed = client
            .sign(signer, signable)
            .await
            .with_context(|| format!("sign {} failed", spec.label))?;

        let order_id = compute_order_id(client, &signed, token_id).await?;

        order_meta.insert(
            spec.label.clone(),
            json!({
                "order_id": order_id,
                "token_id": spec.token,
                "side": format!("{:?}", spec.side),
                "price": spec.price,
                "size": spec.size,
                "order_type": format!("{:?}", spec.order_type),
            }),
        );
        order_id_by_label.insert(spec.label.clone(), order_id);
        signed_by_label.insert(spec.label.clone(), signed);
    }

    if args.dry_run {
        return Ok(json!({
            "run": run_idx,
            "dry_run": true,
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

    // Each spawned task returns (response_arrived_instant, json_or_err).
    type TaskHandle = JoinHandle<(Instant, std::result::Result<Value, String>)>;
    // (step_index, handle)
    let mut inflight: Vec<(usize, TaskHandle)> = Vec::new();
    // step_index → JSON record (filled in as we go; submit/cancel records get t_recv_us + response after join)
    let mut records: Vec<Value> = Vec::with_capacity(args.steps.len());

    for (i, step) in args.steps.iter().enumerate() {
        match step {
            StepSpec::Submit(label) => {
                let signed = signed_by_label.remove(label).ok_or_else(|| {
                    anyhow!("internal: order `{label}` already consumed by an earlier submit")
                })?;
                let client = client.clone();
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
                let client = client.clone();
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

// Ensure the SDK Decimal ↔ rust_decimal::Decimal match (smoke).
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
    fn parse_order_spec_rejects_bad_side() {
        let s = "m1=0xabc:long:0.30:5";
        assert!(parse_order_spec(s).is_err());
    }

    #[test]
    fn parse_order_spec_rejects_too_few_fields() {
        let s = "m1=0xabc:buy:0.30";
        assert!(parse_order_spec(s).is_err());
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
