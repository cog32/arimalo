grammar Ledger;

/*
CRDT-friendly PTA (hledger/ledger-like) with rich crypto lot annotations.

Key ideas:
- Transaction header includes meta comment; semantic pass enforces txn:<ID>.
- Each posting has explicit amount.
- Optional cost annotations:
    { ... }   per-unit cost + optional metadata
    {{ ... }} total cost + optional metadata
- Optional price annotations:
    @  <amount>   per-unit price
    @@ <amount>   total price
- Lot annotation content is structured: comma-separated fields.
  Fields can be positional or key:value.

Examples:
2026-01-15 * "Binance" "Buy SOL" ; txn:01J2N9R9..., src:binance:order:999
    assets:exchange:binance:sol    10.000000 SOL {{ 230.00 USD, fee:0.10 USD, fee_to:expenses:fees:trading, venue:binance, note:"maker fee" }}
    assets:cash:usd              -230.10 USD

2026-01-16T10:05:03Z * "Kraken" "Sell BTC" ; txn:01J2NB..., src:kraken:trade:def456
    assets:exchange:kraken:btc   -0.005000 BTC { 30000 USD, 2026-01-15T09:31:22.123456+11:00, "original acquisition", lot:kraken:btc:abc123 } @@ 160.00 USD
    assets:cash:usd               160.00 USD
    income:trading:pnl            -10.00 USD
*/

file
  : (blankLine | directive | accountDeclaration | transaction)* EOF
  ;

transaction
  : headerLine postingLine+ blankLine*
  ;

accountDeclaration
  : 'account' WS+ account (WS+ commodity)? WS* metaComment? NEWLINE
    accountDeclLine* blankLine*
  ;

accountDeclLine
  : INDENT accountDeclBody WS* metaComment? NEWLINE
  ;

accountDeclBody
  : 'opening' WS+ signedNumber (WS+ commodity)?
  ;

headerLine
  : DATETIME WS+ status? WS* payee? WS* narration? WS* metaComment NEWLINE
  ;

postingLine
  : INDENT account WS+ amountSpec WS*
    lotCostSpec? WS*
    priceSpec? WS*
    metaComment? NEWLINE
  ;

directive
  : DIRECTIVE NEWLINE
  ;

blankLine
  : WS* NEWLINE
  ;

/* ----- Components ----- */

status
  : '*' | '!'
  ;

payee
  : QUOTED
  | WORDS
  ;

narration
  : QUOTED
  | WORDS
  ;

metaComment
  : ';' WS* tagList
  ;

tagList
  : tag (WS* ',' WS* tag)*
  ;

tag
  : key ':' value
  | PATHLIKE
  ;

key
  : IDENT
  ;

value
  : QUOTED
  | PATHLIKE
  | DATETIME
  | NUMBER
  | IDLIKE
  | IDENT
  ;

account
  : PATHLIKE
  ;

amountSpec
  : signedNumber WS+ commodity
  ;

signedNumber
  : SIGN? NUMBER
  ;

commodity
  : IDENT
  ;

/* ----- Cost / Lot annotations ----- */

lotCostSpec
  : costPerUnit
  | costTotal
  ;

/*
Per-unit cost annotation:
  { 30000 USD, date:2026-01-15T..., lot:..., src:..., note:"..." }
  { 30000 USD, 2026-01-15T..., "Bought on dip", lot:abc }
*/
costPerUnit
  : '{' WS* costBody? WS* '}'
  ;

/*
Total cost annotation:
  {{ 300.00 USD, fee:1.50 USD, method:"FIFO", lot:... }}
*/
costTotal
  : '{{' WS* costBody? WS* '}}'
  ;

/*
Cost body begins optionally with a primary cost amount, then any number of extra fields.
We allow either:
- leading amountSpec (common case)
- or only fields (for “unknown cost but metadata known”)
*/
costBody
  : (amountSpec (WS* ',' WS* lotField)*)        #costWithAmount
  | (lotField (WS* ',' WS* lotField)*)          #costFieldsOnly
  ;

/*
A lotField can be:
- key:value (recommended, stable)
- a positional datetime
- a positional quoted label/note
- a positional identifier/pathlike (e.g. exchange:kraken)
- a positional amountSpec (for fee allocations etc.)
*/
lotField
  : lotKV
  | DATETIME
  | QUOTED
  | PATHLIKE
  | IDENT
  | amountSpec
  ;

lotKV
  : lotKey ':' lotValue
  ;

/*
Keys are free-form identifiers, so you can extend without changing grammar:
  date, lot, src, txid, method, venue, fee, fee_in, fee_to, basis, class, etc.
*/
lotKey
  : IDENT
  ;

/*
Values can be rich:
- amountSpec (fee:1.50 USD)
- datetime (date:...)
- quoted strings (note:"...")
- pathlike strings (src:kraken:trade:abc)
- identifiers/numbers
*/
lotValue
  : amountSpec
  | DATETIME
  | QUOTED
  | PATHLIKE
  | NUMBER
  | IDENT
  ;

/* ----- Price annotations ----- */

priceSpec
  : pricePerUnit
  | priceTotal
  ;

pricePerUnit
  : '@' WS* amountSpec
  ;

priceTotal
  : '@@' WS* amountSpec
  ;

/* ----- Lexer ----- */

/*
ISO-ish date or datetime with microseconds (1..6 fractional digits) and optional TZ.
Examples:
  2026-01-15
  2026-01-15T09:31:22
  2026-01-15T09:31:22.123456
  2026-01-15T09:31:22.1Z
  2026-01-15T09:31:22.123+11:00
*/
DATETIME
  : DIGIT DIGIT DIGIT DIGIT '-' DIGIT DIGIT '-' DIGIT DIGIT
    ( 'T'
      DIGIT DIGIT ':' DIGIT DIGIT ':' DIGIT DIGIT
      ('.' DIGIT DIGIT? DIGIT? DIGIT? DIGIT? DIGIT?)?
      ( 'Z' | (('+'|'-') DIGIT DIGIT ':' DIGIT DIGIT) )?
    )?
  ;

INDENT
  : {this.charPositionInLine == 0}? '    '
  ;

SIGN
  : '+' | '-'
  ;

NUMBER
  : DIGIT+ ('.' DIGIT+)?   // 10 or 10.50
  ;

IDENT
  : [A-Za-z_][A-Za-z0-9_]* // BTC, USD, txn, fee, lot
  ;

IDLIKE
  : [A-Za-z0-9][A-Za-z0-9_.-]*
  ;

PATHLIKE
  : PATHSEG ':' PATHSEG (':' PATHSEG)*
  ;

QUOTED
  : '"' ( '\\"' | ~["\r\n] )* '"'
  ;

WORDS
  : WORD (WS+ WORD)*
  ;

fragment WORD
  : [A-Za-z_][A-Za-z0-9_.-/]*
  ;

fragment PATHSEG
  : [A-Za-z0-9_.-]+
  ;

DIRECTIVE
  : {this.charPositionInLine == 0}? [ \t]* ';' ~[\r\n]*
  ;

WS
  : [ \t]+
  ;

NEWLINE
  : '\r'? '\n'
  ;

fragment DIGIT
  : [0-9]
  ;
