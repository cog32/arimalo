# Account Naming Convention

## Context

Current account names are inconsistent. With the new drill-down sidebar, a consistent hierarchy matters because each level becomes a navigable column. This documents the convention for new accounts — existing accounts keep their current names.

## Nomenclature

```
assets:<asset-class>:<custody-type>:<institution>:<account>
```

| Segment | Meaning | Examples |
|---------|---------|---------|
| `assets` | Accounting classification | Always `assets` for things you own |
| `<asset-class>` | What kind of asset | `crypto`, `cash`, `equity` |
| `<custody-type>` | How/where it's held | `exchange`, `wallet`, `bank`, `broker` |
| `<institution>` | Which institution or chain | `binance`, `cba`, `ethereum`, `commsec` |
| `<account>` | Specific account (optional) | `personal`, `savings`, `0x6d25...` |

Not every account needs all levels — stop at the leaf that makes sense.

## Full Account Map

### Crypto — Exchanges
```
assets:crypto:exchange:binance:personal
assets:crypto:exchange:bybit:personal
assets:crypto:exchange:kraken:personal
```

### Crypto — Self-custody Wallets
```
assets:crypto:wallet:ethereum:0x6d25d07f5c0dccd0d6c7b3342cd83b902464f06b
assets:crypto:wallet:ethereum:0xd2925983502b2f849c96dbe449179e8b09d8c6a7
assets:crypto:wallet:solana:2baaTDzidWekQWQydZcBSwHXgqK1LF9QUUn7VioUrVVD
assets:crypto:wallet:solana:HgspjimVL6zisiEVTStrbpA4D9D8go4GnTjCFJontaC9
assets:crypto:wallet:bitcoin
```

### Cash — Banks
```
assets:cash:bank:cba:smartaccess
assets:cash:bank:cba:goalsaver
assets:cash:bank:cba:cdia
assets:cash:bank:ubank:savings
```

### Equity — Brokerage
```
assets:equity:broker:commsec:personal
```

## Drill-Down Navigation

```
assets
  ├── crypto ›
  │     ├── exchange ›
  │     │     ├── binance ›
  │     │     │     └── personal
  │     │     ├── bybit ›
  │     │     │     └── personal
  │     │     └── kraken ›
  │     │           └── personal
  │     └── wallet ›
  │           ├── bitcoin
  │           ├── ethereum ›
  │           │     ├── 0x6d25...
  │           │     └── 0xd292...
  │           └── solana ›
  │                 ├── 2baaT...
  │                 └── Hgspj...
  ├── cash ›
  │     └── bank ›
  │           ├── cba ›
  │           │     ├── cdia
  │           │     ├── goalsaver
  │           │     └── smartaccess
  │           └── ubank ›
  │                 └── savings
  └── equity ›
        └── broker ›
              └── commsec ›
                    └── personal
```

## Corresponding Folder Structure (for new accounts)

```
sources/richard/
  crypto/
    exchange/
      binance/personal/
      bybit/personal/
      kraken/personal/
    wallet/
      bitcoin/
      ethereum/0x.../
      solana/addr.../
  cash/
    bank/
      cba/cdia/
      cba/goalsaver/
      cba/smartaccess/
      ubank/savings/
  equity/
    broker/
      commsec/personal/
```
