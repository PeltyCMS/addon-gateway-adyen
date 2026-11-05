//! Adyen-style generic redirect gateway: Pay by Link (`POST /paymentLinks`,
//! `X-API-Key`), standard notifications with per-item HMAC
//! (`additionalData.hmacSignature` = base64(HMAC-SHA256 with the hex-decoded
//! key over the escaped, colon-joined fields)). `provider_ref` is our
//! merchant reference (payment id) so notifications map back directly.
//! Refunds need the PSP reference and are not wired. Mock-tested only.
use base64::Engine;
use pelty_payments::http::{api_base, ct_eq, hmac_sha256, send_json};
use pelty_payments::*;
#[allow(unused_imports)]
use {
    pelty_core::money::Money,
    serde::{Deserialize, Serialize},
    std::collections::{BTreeMap, HashMap},
    std::future::Future,
    std::pin::Pin,
    std::sync::Arc,
};

pub const API_BASE: &str = "https://checkout-test.adyen.com/v71";

pub struct AdyenGateway {
    key: String,
    hmac_key: Vec<u8>,
    merchant: String,
    base: String,
    t: Arc<dyn HttpTransport>,
}

const SETTINGS: &[SettingField] = &[
    SettingField::text("merchant_account", true),
    SettingField::of("api_base", "url"),
];

pub fn descriptor() -> GatewayDescriptor {
    GatewayDescriptor {
        id: "adyen",
        name: "Adyen-style redirect",
        capabilities: Capabilities {
            checkout: true,
            webhooks: true,
            refunds: false,
            recurring: false,
        },
        secrets: &["api_key", "hmac_key"],
        settings: SETTINGS,
        currencies: &[],
    }
}

pub fn factory(cfg: &GatewayConfig) -> Result<Box<dyn PaymentGateway>, GatewayError> {
    Ok(Box::new(AdyenGateway {
        key: cfg.secret("api_key")?.into(),
        hmac_key: hex::decode(cfg.secret("hmac_key")?)
            .map_err(|_| GatewayError::Config("hmac_key must be hex".into()))?,
        merchant: cfg
            .str("merchant_account")
            .filter(|s| !s.is_empty())
            .ok_or_else(|| GatewayError::Config("merchant_account".into()))?
            .into(),
        // Test endpoint unless the operator sets the live URL explicitly.
        base: api_base(&cfg.config, API_BASE)?,
        t: cfg.transport.clone(),
    }))
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace(':', "\\:")
}

/// Signing string for one NotificationRequestItem.
pub fn signing_string(i: &serde_json::Value) -> String {
    let f = |p: &str| obj_str(i, p).unwrap_or_default();
    [
        f("pspReference"),
        f("originalReference"),
        f("merchantAccountCode"),
        f("merchantReference"),
        f("amount.value"),
        f("amount.currency"),
        f("eventCode"),
        f("success"),
    ]
    .iter()
    .map(|s| esc(s))
    .collect::<Vec<_>>()
    .join(":")
}

impl PaymentGateway for AdyenGateway {
    fn id(&self) -> &'static str {
        "adyen"
    }
    fn display_name(&self) -> &'static str {
        "Adyen"
    }
    fn capabilities(&self) -> Capabilities {
        descriptor().capabilities
    }
    fn create_payment<'a>(
        &'a self,
        r: &'a PaymentRequest,
    ) -> BoxFuture<'a, Result<PaymentInit, GatewayError>> {
        Box::pin(async move {
            let body = serde_json::json!({
                "reference": r.payment_id,
                "amount": {"value": minor_units(r.amount)?, "currency": r.amount.currency.as_str()},
                "merchantAccount": self.merchant,
                "returnUrl": r.success_url,
                "description": r.description,
                "shopperEmail": r.customer_email,
            });
            let spec = HttpRequestSpec {
                method: "POST",
                url: format!("{}/paymentLinks", self.base),
                headers: vec![
                    ("X-API-Key".into(), self.key.clone()),
                    ("Content-Type".into(), "application/json".into()),
                    ("Idempotency-Key".into(), r.idempotency_key.clone()),
                ],
                body: body.to_string(),
            };
            let v = send_json(self.t.as_ref(), spec).await?;
            let url = v["url"]
                .as_str()
                .ok_or_else(|| GatewayError::Provider("missing url".into()))?;
            Ok(PaymentInit::Redirect {
                url: url.into(),
                provider_ref: r.payment_id.clone(),
            })
        })
    }
    fn parse_webhook(
        &self,
        _h: &Headers,
        body: &[u8],
        _now: i64,
    ) -> Result<WebhookEvent, GatewayError> {
        let v: serde_json::Value =
            serde_json::from_slice(body).map_err(|_| GatewayError::MalformedPayload)?;
        let item = &v["notificationItems"][0]["NotificationRequestItem"];
        if !item.is_object() {
            return Err(GatewayError::MalformedPayload);
        }
        let sig =
            obj_str(item, "additionalData.hmacSignature").ok_or(GatewayError::InvalidSignature)?;
        let want = hmac_sha256(&self.hmac_key, signing_string(item).as_bytes());
        let got = base64::engine::general_purpose::STANDARD
            .decode(sig)
            .map_err(|_| GatewayError::InvalidSignature)?;
        if !ct_eq(&got, &want) {
            return Err(GatewayError::InvalidSignature);
        }
        let code = obj_str(item, "eventCode").unwrap_or_default();
        let ok = obj_str(item, "success").as_deref() == Some("true");
        let kind = match (code.as_str(), ok) {
            ("AUTHORISATION", true) => WebhookKind::PaymentSucceeded,
            ("AUTHORISATION", false) => WebhookKind::PaymentFailed,
            ("REFUND", true) => WebhookKind::Refunded,
            ("CHARGEBACK", _) | ("NOTIFICATION_OF_CHARGEBACK", _) => WebhookKind::Disputed,
            _ => WebhookKind::Ignored,
        };
        let psp = obj_str(item, "pspReference").ok_or(GatewayError::MalformedPayload)?;
        let r = obj_str(item, "merchantReference");
        let mut e = WebhookEvent::new(format!("{psp}:{code}:{ok}"), code, kind, r.clone());
        e.payment_id = r;
        Ok(e)
    }
    fn refund<'a>(
        &'a self,
        _: &'a str,
        _: Money,
        _: &'a str,
    ) -> BoxFuture<'a, Result<RefundResult, GatewayError>> {
        Box::pin(async { Err(GatewayError::Unsupported) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pelty_payments::test_util::*;

    const KEY: &str = "44782DEF547AAA06C910C43932B1EB0C71FC68D9D0C057550C48EC2ACF6BA056";

    #[tokio::test]
    async fn link_and_hmac_notification() {
        let t = MockTransport::new();
        let gw = factory(&cfg(
            "adyen",
            serde_json::json!({"merchant_account": "TestMerchant"}),
            &[("api_key", "AQE"), ("hmac_key", KEY)],
            &t,
        ))
        .unwrap();
        t.push(201, r#"{"id":"PL1","url":"https://test.adyen.link/PL1"}"#);
        assert_eq!(
            gw.create_payment(&req("10.5", "EUR")).await.unwrap(),
            PaymentInit::Redirect {
                url: "https://test.adyen.link/PL1".into(),
                provider_ref: "pay_1".into()
            }
        );
        let v: serde_json::Value = serde_json::from_str(&t.requests()[0].body).unwrap();
        assert_eq!(v["amount"]["value"], 1050);
        assert!(t.requests()[0]
            .url
            .starts_with("https://checkout-test.adyen.com"));

        // Signature computed independently in Python:
        // base64(hmac(bytes.fromhex(KEY), b"7914073381342284::TestMerchant:pay_1:1050:EUR:AUTHORISATION:true", sha256))
        let sig = "6JAYqENwSbDKqOxEv30u+mCNiKBur7xF1Et9vViwkK4=";
        let body = format!(
            r#"{{"live":"false","notificationItems":[{{"NotificationRequestItem":{{"additionalData":{{"hmacSignature":"{sig}"}},"amount":{{"currency":"EUR","value":1050}},"eventCode":"AUTHORISATION","merchantAccountCode":"TestMerchant","merchantReference":"pay_1","pspReference":"7914073381342284","success":"true"}}}}]}}"#
        );
        let e = gw
            .parse_webhook(&Headers::new(), body.as_bytes(), 0)
            .unwrap();
        assert_eq!(
            (e.kind, e.provider_ref.as_deref()),
            (WebhookKind::PaymentSucceeded, Some("pay_1"))
        );
        assert_eq!(
            gw.parse_webhook(&Headers::new(), body.replace("1050", "1").as_bytes(), 0)
                .unwrap_err(),
            GatewayError::InvalidSignature
        );
    }

    #[test]
    fn escapes_colons_and_backslashes() {
        let i = serde_json::json!({"pspReference": "a:b", "merchantReference": "c\\d", "amount": {"value": 1, "currency": "EUR"}, "eventCode": "X", "success": "true"});
        assert_eq!(signing_string(&i), "a\\:b:::c\\\\d:1:EUR:X:true");
    }
}
