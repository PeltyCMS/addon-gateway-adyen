//! Adyen-style redirect gateway addon: Payment links with HMAC notifications.
//! The whole gateway lives in [`adyen`]; this file only exports it to the host.
pub mod adyen;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::gateway_addon!(adyen::descriptor, adyen::factory);
