# Adyen

Accept Adyen payments in PeltyCMS checkout

Official payment gateway for [PeltyCMS](https://github.com/MythicalLTD/PeltyCMS).

## Why this addon

Hosts who want customers to pay invoices without leaving Pelty checkout.

## Features

- Take invoice and checkout payments through **Adyen**
- Signed webhook handling so paid invoices mark paid automatically
- Fits Pelty tax, credits, refunds and multi-currency
- Runs as sandboxed WASM — no custom PHP on your panel
- Keys live in Pelty secrets, not in theme code

## How it works

1. Install **Adyen** from the [Mythical marketplace](https://market.mythicalsystems.org/marketplace/product/gateway-adyen) inside Pelty Admin → Addons.
2. Open Pelty Admin → Payments, add your Adyen keys, and enable the gateway on the products you sell.
3. Customers hit checkout / login / order flow as usual — Adyen runs in the background.

## Requirements

- PeltyCMS with WASM addons enabled
- Working Adyen credentials
- Outbound HTTPS from your panel

## Get it

**[Adyen on Mythical Marketplace](https://market.mythicalsystems.org/marketplace/product/gateway-adyen)**

## Support

Bugs and questions for this addon → Issues on this repo.  
PeltyCMS core → https://github.com/MythicalLTD/PeltyCMS
