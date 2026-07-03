CRDT Integration Plan for Multi-Machine Account Synchronization                │
     │                                                                                │
     │ Context                                                                        │
     │                                                                                │
     │ The user wants to enable multi-machine synchronization for their accounting    │
     │ application using CRDTs (Conflict-free Replicated Data Types) like Automerge.  │
     │ Currently, the application is strictly local-first with file-based storage,    │
     │ with no synchronization mechanism between machines.                            │
     │                                                                                │
     │ Current Architecture:                                                          │
     │ - Plain text ledger format (inspired by hledger/ledger)                        │
     │ - File-based storage in app data directories                                   │
     │ - Deterministic transaction IDs (txn: tags in metadata)                        │
     │ - Append-only transaction model                                                │
     │ - Build cache for CSV transformations (hash-based)                             │
     │ - No network sync, no conflict resolution                                      │
     │                                                                                │
     │ Why This Change:                                                               │
     │ To enable the user to work on their accounts from multiple machines (e.g.,     │
     │ desktop and laptop) with automatic synchronization and conflict-free merging of│
     │  changes made on different devices.                                            │
     │                                                                                │
     │ ---                                                                            │
     │ Exploration Findings                                                           │
     │                                                                                │
     │ Current Data Model                                                             │
     │                                                                                │
     │ Transaction Structure:                                                         │
     │ pub struct Transaction {                                                       │
     │   pub date: String,              // YYYY-MM-DD                                 │
     │   pub datetime: String,          // ISO 8601                                   │
     │   pub status: Option<char>,      // '*' (cleared) or '!' (pending)             │
     │   pub payee: Option<String>,                                                   │
     │   pub narration: Option<String>,                                               │
     │   pub meta: Option<String>,      // txn:csv-abc123, origin:device-id, etc.     │
     │   pub postings: Vec<Posting>,                                                  │
     │ }                                                                              │
     │                                                                                │
     │ Storage Structure:                                                             │
     │ sources/                                                                       │
     │   ├── manual.transactions          # User-added transactions                   │
     │   ├── accounts.transactions        # Account declarations                      │
     │   ├── {account}/                                                               │
     │   │   ├── *.csv                   # Bank imports                               │
     │   │   ├── _transform.rhai         # CSV transform script                       │
     │   │   └── _rules.json             # Categorization rules                       │
     │                                                                                │
     │ generated/                                                                     │
     │   ├── ledger.transactions         # Active ledger                              │
     │   ├── build-cache.json            # CSV transformation cache                   │
     │   └── archive/                                                                 │
     │       └── ledger-{YYYYMM}.transactions                                         │
     │                                                                                │
     │ CRDT-Friendly Aspects                                                          │
     │                                                                                │
     │ ✅ Already Present:                                                            │
     │ 1. Deterministic transaction IDs (SHA256-based)                                │
     │ 2. Append-only transaction semantics                                           │
     │ 3. Plain text format (mergeable)                                               │
     │ 4. Immutable transactions                                                      │
     │ 5. Grammar hints at CRDT intentions (txn:<ID> for traceability)                │
     │                                                                                │
     │ ❌ Missing:                                                                    │
     │ 1. No change tracking or versioning                                            │
     │ 2. No device origin tracking                                                   │
     │ 3. No conflict resolution                                                      │
     │ 4. No sync protocol                                                            │
     │ 5. Build cache is per-machine (can't be synced)                                │
     │                                                                                │
     │ Critical Files                                                                 │
     │                                                                                │
     │ - src-tauri/src/ledger_parser.rs (517 lines) - Parse ledger format             │
     │ - src-tauri/src/generated_store.rs (170 lines) - Load/write ledger files       │
     │ - src-tauri/src/processing_pipeline.rs (380+ lines) - CSV transformation pipeline     │
     │ - src-tauri/src/build_cache.rs (77 lines) - Build cache management             │
     │ - src-tauri/src/rules.rs (181 lines) - Transaction rules                       │
     │                                                                                │
     │ ---                                                                            │
     │ Proposed Approaches                                                            │
     │                                                                                │
     │ [To be filled after user feedback on preferred direction]                      │
     │                                                                                │
     │ Approach 1: Automerge Document-Based Sync                                      │
     │                                                                                │
     │ Concept: Replace file-based storage with Automerge documents                   │
     │                                                                                │
     │ Pros:                                                                          │
     │ - Automatic conflict resolution                                                │
     │ - Built-in sync protocol                                                       │
     │ - Rich data types (text, lists, maps)                                          │
     │ - Change tracking built-in                                                     │
     │                                                                                │
     │ Cons:                                                                          │
     │ - Significant architectural change                                             │
     │ - Loss of plain text ledger format                                             │
     │ - New dependency (automerge-rs)                                                │
     │ - Export/import needed for plain text compatibility                            │
     │                                                                                │
     │ Approach 2: CRDT Layer Over Plain Text                                         │
     │                                                                                │
     │ Concept: Keep plain text ledger, add CRDT metadata layer                       │
     │                                                                                │
     │ Pros:                                                                          │
     │ - Preserves existing format                                                    │
     │ - Human-readable files remain                                                  │
     │ - Minimal disruption to existing code                                          │
     │ - Can use tools like hledger/ledger                                            │
     │                                                                                │
     │ Cons:                                                                          │
     │ - More complex merge logic                                                     │
     │ - Need to manage both file and CRDT state                                      │
     │ - Risk of divergence between layers                                            │
     │                                                                                │
     │ Approach 3: Git-Based Sync with CRDT Metadata                                  │
     │                                                                                │
     │ Concept: Use git for file sync, add CRDT metadata for conflict resolution      │
     │                                                                                │
     │ Pros:                                                                          │
     │ - Leverage git's proven sync                                                   │
     │ - Audit trail via git history                                                  │
     │ - Familiar tooling                                                             │
     │ - Plain text preserved                                                         │
     │                                                                                │
     │ Cons:                                                                          │
     │ - Manual conflict resolution still needed                                      │
     │ - Git merge conflicts require handling                                         │
     │ - Not fully automatic                                                          │
     │                                                                                │
     │ Approach 4: Custom Event Log with CRDT                                         │
     │                                                                                │
     │ Concept: Event sourcing with CRDT-based event log                              │
     │                                                                                │
     │ Pros:                                                                          │
     │ - Complete audit trail                                                         │
     │ - Can rebuild state from events                                                │
     │ - Natural fit for accounting                                                   │
     │ - Device-specific views                                                        │
     │                                                                                │
     │ Cons:                                                                          │
     │ - Most complex implementation                                                  │
     │ - Need to design event schema                                                  │
     │ - Migration path unclear                                                       │
     │ - Performance overhead                                                         │
     │                                                                                │
     │ ---                                                                            │
     │ User Requirements                                                              │
     │                                                                                │
     │ Based on user feedback:                                                        │
     │                                                                                │
     │ 1. Sync Method:                                                                │
     │   - Interested in Automerge and similar CRDT tools                             │
     │   - Wants git-like approach but local                                          │
     │   - All data types (transactions, PDFs, etc.)                                  │
     │ 2. Format Preference:                                                          │
     │   - Prefers plain text but flexible                                            │
     │   - Open to binary format if needed                                            │
     │ 3. Critical Requirement:                                                       │
     │   - NO UNEXPLAINED CHANGES - must have audit trail                             │
     │   - Wants to understand what changed and why                                   │
     │ 4. Data Scope:                                                                 │
     │   - Everything needs to sync (transactions, rules, CSVs, PDFs, etc.)           │
     │   - Not just transactions                                                      │
     │                                                                                │
     │ ---                                                                            │
     │ Automerge Integration Options                                                  │
     │                                                                                │
     │ What is Automerge?                                                             │
     │                                                                                │
     │ Automerge is a CRDT library that:                                              │
     │ - Automatically merges concurrent edits from different devices                 │
     │ - Maintains complete change history (audit trail)                              │
     │ - Works offline-first                                                          │
     │ - Syncs via simple save/load or network protocol                               │
     │ - Supports rich data types (text, lists, maps, binary)                         │
     │                                                                                │
     │ Rust Support: automerge crate available, actively maintained                   │
     │                                                                                │
     │ Where Automerge Fits                                                           │
     │                                                                                │
     │ Option 1: Automerge as Primary Data Store                                      │
     │ - Replace file-based storage with Automerge documents                          │
     │ - Each device has local Automerge replica                                      │
     │ - Sync via file sharing (Dropbox, Syncthing) or network                        │
     │ - Export to plain text ledger for external tools                               │
     │                                                                                │
     │ Option 2: Automerge for Metadata + Files                                       │
     │ - Keep large files (CSVs, PDFs) as files                                       │
     │ - Use Automerge for transaction metadata and indices                           │
     │ - Hybrid: structured data in Automerge, blobs as files                         │
     │ - Best of both worlds                                                          │
     │                                                                                │
     │ Option 3: Git-like with Automerge                                              │
     │ - Automerge provides the CRDT merge logic                                      │
     │ - File-based commits (like git)                                                │
     │ - Complete audit trail via Automerge history                                   │
     │ - Plain text export for inspection                                             │
     │                                                                                │
     │ ---                                                                            │
     │ ---                                                                            │
     │ Detailed Implementation Options                                                │
     │                                                                                │
     │ Option 1: Automerge as Primary Data Store                                      │
     │                                                                                │
     │ Architecture:                                                                  │
     │ - Replace file-based storage with Automerge binary documents                   │
     │ - Single source of truth: arimalo.automerge file                              │
     │ - Export to plain text on demand for external tools                            │
     │ - All data (transactions, rules, CSVs, PDFs) stored in Automerge               │
     │                                                                                │
     │ Key Schema:                                                                    │
     │ struct ArimaloDoc {                                                           │
     │     transactions: automerge::List<TransactionMap>,  // All transactions        │
     │     accounts: automerge::Map<String, AccountMap>,   // Account declarations    │
     │     rules: automerge::Map<String, RuleMap>,         // Categorization rules    │
     │     transforms: automerge::Map<String, String>,     // Rhai scripts            │
     │     csv_files: automerge::Map<String, CsvBlob>,     // CSV content             │
     │     metadata: MetadataMap,                          // Device tracking         │
     │ }                                                                              │
     │                                                                                │
     │ struct TransactionMap {                                                        │
     │     id: String,              // txn:abc123 (deterministic)                     │
     │     datetime: String,                                                          │
     │     postings: List<PostingMap>,                                                │
     │     device_origin: String,   // Which device created it                        │
     │     created_at: i64,                                                           │
     │     // ... other fields                                                        │
     │ }                                                                              │
     │                                                                                │
     │ Sync Protocol:                                                                 │
     │ - File-based: Copy arimalo.automerge between machines                         │
     │ - Automerge automatically merges concurrent edits                              │
     │ - Incremental sync via sync messages (smaller than full doc)                   │
     │                                                                                │
     │ Audit Trail:                                                                   │
     │ // Get all changes to document                                                 │
     │ let changes = doc.get_changes(&[]);                                            │
     │ for change in changes {                                                        │
     │     println!("Device: {}", change.actor_id());                                 │
     │     println!("Timestamp: {}", change.timestamp());                             │
     │     // See exact operations: insert, set, delete                               │
     │ }                                                                              │
     │                                                                                │
     │ Pros:                                                                          │
     │ - True CRDT with strong guarantees                                             │
     │ - Built-in change history                                                      │
     │ - Automatic conflict resolution                                                │
     │ - Single source of truth                                                       │
     │                                                                                │
     │ Cons:                                                                          │
     │ - Large binary format for CSV/PDF files (storage overhead)                     │
     │ - Plain text becomes secondary (export needed)                                 │
     │ - Higher migration risk                                                        │
     │ - Performance concerns with large datasets                                     │
     │                                                                                │
     │ Complexity: 4-6 weeks                                                          │
     │                                                                                │
     │ ---                                                                            │
     │ Option 2: Hybrid (Automerge + Files) ⭐ RECOMMENDED                            │
     │                                                                                │
     │ Architecture:                                                                  │
     │ - Automerge for metadata (transaction refs, accounts, rules)                   │
     │ - Files for content (CSVs, PDFs, transform scripts)                            │
     │ - Content-addressed storage for deduplication                                  │
     │ - Plain text files remain primary                                              │
     │                                                                                │
     │ Key Schema:                                                                    │
     │ struct HybridDoc {                                                             │
     │     // References to transactions in files                                     │
     │     transaction_refs: automerge::List<TxnRef>,                                 │
     │                                                                                │
     │     // Small structured data                                                   │
     │     accounts: automerge::Map<String, AccountMap>,                              │
     │     rules: automerge::Map<String, RuleMap>,                                    │
     │                                                                                │
     │     // File manifest (content hash → path)                                     │
     │     file_manifest: automerge::Map<String, FileEntry>,                          │
     │                                                                                │
     │     // Audit trail                                                             │
     │     sync_log: automerge::List<SyncEvent>,                                      │
     │ }                                                                              │
     │                                                                                │
     │ struct TxnRef {                                                                │
     │     id: String,              // txn:abc123                                     │
     │     content_hash: String,    // SHA-256 of transaction text                    │
     │     file_path: String,       // Where it lives                                 │
     │     device_origin: String,                                                     │
     │ }                                                                              │
     │                                                                                │
     │ struct FileEntry {                                                             │
     │     content_hash: String,    // SHA-256 for deduplication                      │
     │     relative_path: String,                                                     │
     │     file_type: String,       // "csv", "pdf", "transform"                      │
     │     device_origin: String,                                                     │
     │ }                                                                              │
     │                                                                                │
     │ Content Addressing:                                                            │
     │ sources/                                                                       │
     │   manual.transactions          # Plain text, editable                          │
     │   bank_a/                                                                      │
     │     statement.csv             # Or symlink to cas/ab/cdef...                   │
     │     _transform.rhai           # Plain text code                                │
     │                                                                                │
     │ cas/                           # Content-addressed storage                     │
     │   ab/                                                                          │
     │     cdef123456...             # Actual file content (by hash)                  │
     │                                                                                │
     │ Sync Protocol (Two-Phase):                                                     │
     │ 1. Phase 1: Sync Automerge metadata doc                                        │
     │ 2. Phase 2: Compare file manifests, transfer missing files by hash             │
     │                                                                                │
     │ Audit Trail:                                                                   │
     │ - Sync log tracks all file operations                                          │
     │ - File versioning via content hashing                                          │
     │ - Transaction history via TxnRef changes                                       │
     │                                                                                │
     │ Pros:                                                                          │
     │ - ✅ Plain text files remain primary (File Over App)                           │
     │ - ✅ External tools work directly (hledger, grep, etc.)                        │
     │ - ✅ Better for large files (CSVs, PDFs)                                       │
     │ - ✅ Lowest migration risk                                                     │
     │ - ✅ CRDT benefits for metadata                                                │
     │ - ✅ Familiar file-based workflow                                              │
     │                                                                                │
     │ Cons:                                                                          │
     │ - Two systems to maintain (Automerge + files)                                  │
     │ - Sync is two-phase (more complex)                                             │
     │ - Potential for metadata/file drift                                            │
     │                                                                                │
     │ Complexity: 3-4 weeks                                                          │
     │                                                                                │
     │ Why Recommended:                                                               │
     │ - Best balance of CRDT benefits and plain text preservation                    │
     │ - Lowest risk migration (Automerge layer is additive)                          │
     │ - Practical for all data types (small in Automerge, large as files)            │
     │ - Preserves "File Over App" philosophy                                         │
     │ - Strong audit trail via sync log                                              │
     │                                                                                │
     │ ---                                                                            │
     │ Option 3: Git-like Architecture with Automerge                                 │
     │                                                                                │
     │ Architecture:                                                                  │
     │ - Git concepts: commits, branches, trees, blobs                                │
     │ - Each device = branch                                                         │
     │ - Commits = snapshots of entire state                                          │
     │ - Automerge provides merge algorithm                                           │
     │                                                                                │
     │ Key Schema:                                                                    │
     │ struct GitLikeDoc {                                                            │
     │     commits: automerge::List<CommitMap>,                                       │
     │     branches: automerge::Map<DeviceId, CommitHash>,  // Device = branch        │
     │     objects: automerge::Map<Hash, Object>,           // Content store          │
     │ }                                                                              │
     │                                                                                │
     │ struct CommitMap {                                                             │
     │     hash: String,                                                              │
     │     parent_hashes: Vec<String>,  // Merge = multiple parents                   │
     │     tree_hash: String,           // Root directory                             │
     │     author_device: String,                                                     │
     │     timestamp: i64,                                                            │
     │     message: String,                                                           │
     │ }                                                                              │
     │                                                                                │
     │ // All data stored as blobs (transactions, CSVs, PDFs, etc.)                   │
     │                                                                                │
     │ Workflow:                                                                      │
     │ // Create commit (snapshot)                                                    │
     │ commit_changes(doc, "Added 5 transactions from bank_a")?;                      │
     │                                                                                │
     │ // Merge another device's branch                                               │
     │ merge_device(doc, "laptop-alice")?;                                            │
     │                                                                                │
     │ // Time travel                                                                 │
     │ checkout_commit(doc, "abc123", working_dir)?;                                  │
     │                                                                                │
     │ Audit Trail:                                                                   │
     │ // Full git-like history                                                       │
     │ log_commits(doc, None)?;          // All commits                               │
     │ diff_commits(doc, "from", "to")?; // Diff between commits                      │
     │ blame_transaction(doc, txn_id)?;  // Who changed what                          │
     │                                                                                │
     │ Pros:                                                                          │
     │ - Excellent audit trail (best of all options)                                  │
     │ - Git-like workflow (familiar to developers)                                   │
     │ - Full version control with time travel                                        │
     │ - Automerge handles merge logic                                                │
     │ - Complete history                                                             │
     │                                                                                │
     │ Cons:                                                                          │
     │ - Most complex implementation (6-8 weeks)                                      │
     │ - Highest learning curve                                                       │
     │ - Storage overhead (all history kept)                                          │
     │ - Overkill for simple sync use case                                            │
     │ - Slow for large files without LFS-like extension                              │
     │                                                                                │
     │ ---                                                                            │
     │ Comparison Matrix                                                              │
     │ ┌────────────────────┬───────────────────────┬───────────────────┬─────────────│
     │ ─────┐                                                                         │
     │ │      Feature       │    Option 1: Pure     │ Option 2: Hybrid  │    Option 3:│
     │      │                                                                         │
     │ │                    │       Automerge       │        ⭐         │     Git-like│
     │      │                                                                         │
     │ ├────────────────────┼───────────────────────┼───────────────────┼─────────────│
     │ ─────┤                                                                         │
     │ │ Plain Text Primary │ No (export only)      │ Yes               │ Yes (working│
     │      │                                                                         │
     │ │                    │                       │                   │ dir)        │
     │      │                                                                         │
     │ ├────────────────────┼───────────────────────┼───────────────────┼─────────────│
     │ ─────┤                                                                         │
     │ │ Audit Trail        │ Good                  │ Medium            │ Excellent   │
     │      │                                                                         │
     │ ├────────────────────┼───────────────────────┼───────────────────┼─────────────│
     │ ─────┤                                                                         │
     │ │ External Tool      │ Medium                │ Excellent         │ Excellent   │
     │      │                                                                         │
     │ │ Compat             │                       │                   │             │
     │      │                                                                         │
     │ ├────────────────────┼───────────────────────┼───────────────────┼─────────────│
     │ ─────┤                                                                         │
     │ │ Large File         │ Poor                  │ Excellent         │ Medium      │
     │      │                                                                         │
     │ │ Handling           │                       │                   │             │
     │      │                                                                         │
     │ ├────────────────────┼───────────────────────┼───────────────────┼─────────────│
     │ ─────┤                                                                         │
     │ │ Sync Complexity    │ Low                   │ Medium            │ High        │
     │      │                                                                         │
     │ ├────────────────────┼───────────────────────┼───────────────────┼─────────────│
     │ ─────┤                                                                         │
     │ │ Implementation     │ 4-6 weeks             │ 3-4 weeks         │ 6-8 weeks   │
     │      │                                                                         │
     │ │ Time               │                       │                   │             │
     │      │                                                                         │
     │ ├────────────────────┼───────────────────────┼───────────────────┼─────────────│
     │ ─────┤                                                                         │
     │ │ Learning Curve     │ Medium                │ Low               │ High        │
     │      │                                                                         │
     │ ├────────────────────┼───────────────────────┼───────────────────┼─────────────│
     │ ─────┤                                                                         │
     │ │ Migration Risk     │ High                  │ Low               │ High        │
     │      │                                                                         │
     │ └────────────────────┴───────────────────────┴───────────────────┴─────────────│
     │ ─────┘                                                                         │
     │                                                                                │
     │                                                                                │
     │                                                                                │
     │                                                                                │
     │                                                                                │
     │                                                                                │
     │                                                                                │
     │                                                                                │
     │                                                                                │
     │                                                                                │
     │                                                                                │
     │ ---                                                                            │
     │ Recommended Implementation Plan: Option 2 (Hybrid)                             │
     │                                                                                │
     │ Phase 1: Metadata Layer (Weeks 1-2)                                            │
     │                                                                                │
     │ Goal: Create Automerge document for structured metadata                        │
     │                                                                                │
     │ Tasks:                                                                         │
     │ 1. Add automerge dependency to Cargo.toml                                      │
     │ 2. Create src-tauri/src/automerge_store.rs module                              │
     │ 3. Define schemas for HybridDoc, TxnRef, FileEntry, SyncEvent                  │
     │ 4. Implement document initialization from existing files                       │
     │ 5. Generate transaction refs from parsed ledger files                          │
     │ 6. Build file manifest from sources directory                                  │
     │                                                                                │
     │ Files to modify:                                                               │
     │ - src-tauri/Cargo.toml - Add automerge dependency                              │
     │ - src-tauri/src/lib.rs - Register new module                                   │
     │ - src-tauri/src/automerge_store.rs - New file for Automerge logic              │
     │ - src-tauri/src/generated_store.rs - Adapt to work with metadata layer         │
     │                                                                                │
     │ Verification:                                                                  │
     │ - Unit tests for Automerge document creation                                   │
     │ - Verify all existing transactions have refs                                   │
     │ - Confirm file manifest completeness                                           │
     │                                                                                │
     │ Phase 2: Content Addressing (Weeks 2-3)                                        │
     │                                                                                │
     │ Goal: Implement content-addressed storage for large files                      │
     │                                                                                │
     │ Tasks:                                                                         │
     │ 1. Create cas/ directory structure for content-addressed files                 │
     │ 2. Implement hash-based file storage                                           │
     │ 3. Add deduplication logic (same hash = same content)                          │
     │ 4. Create file manifest sync                                                   │
     │ 5. Integrity verification (hash checks)                                        │
     │                                                                                │
     │ Files to modify:                                                               │
     │ - src-tauri/src/processing_pipeline.rs - Integrate with CAS for CSV files             │
     │ - src-tauri/src/automerge_store.rs - CAS management functions                  │
     │                                                                                │
     │ Verification:                                                                  │
     │ - Test file deduplication (same CSV on both machines = one copy)               │
     │ - Verify hash integrity checks                                                 │
     │ - Test file retrieval from CAS                                                 │
     │                                                                                │
     │ Phase 3: Sync Protocol (Weeks 3-4)                                             │
     │                                                                                │
     │ Goal: Implement two-phase sync                                                 │
     │                                                                                │
     │ Tasks:                                                                         │
     │ 1. Automerge metadata sync (generate sync messages)                            │
     │ 2. File sync by hash (compare manifests, transfer missing)                     │
     │ 3. Conflict detection and resolution UI                                        │
     │ 4. Sync state tracking (last sync timestamp, device list)                      │
     │ 5. Add Tauri commands: sync_metadata(), sync_files(), list_devices()           │
     │                                                                                │
     │ Files to modify:                                                               │
     │ - src-tauri/src/main.rs - Add sync commands                                    │
     │ - src-tauri/src/automerge_store.rs - Sync protocol implementation              │
     │ - src/main.ts - UI for sync operations                                         │
     │                                                                                │
     │ Verification:                                                                  │
     │ - Test sync between two local directories (simulated devices)                  │
     │ - Test conflict scenarios (same transaction edited on both)                    │
     │ - Verify sync log captures all changes                                         │
     │                                                                                │
     │ Phase 4: Audit Trail UI (Week 4+)                                              │
     │                                                                                │
     │ Goal: Show users what changed and why                                          │
     │                                                                                │
     │ Tasks:                                                                         │
     │ 1. Sync log viewer (timeline of changes)                                       │
     │ 2. Device list (show all known devices)                                        │
     │ 3. Change details (who, what, when, where)                                     │
     │ 4. Conflict resolution UI                                                      │
     │ 5. Export sync log to JSON/CSV for analysis                                    │
     │                                                                                │
     │ Files to modify:                                                               │
     │ - src/main.ts - UI components for audit trail                                  │
     │ - src-tauri/src/main.rs - Commands for querying sync log                       │
     │                                                                                │
     │ Verification:                                                                  │
     │ - User can see all sync events                                                 │
     │ - Filter by device, date, operation type                                       │
     │ - Export works correctly                                                       │
     │                                                                                │
     │ ---                                                                            │
     │ Migration Strategy                                                             │
     │                                                                                │
     │ Backward Compatibility                                                         │
     │                                                                                │
     │ Phase 1: Dual Mode                                                             │
     │ - Keep existing file-based operations working                                  │
     │ - Automerge metadata is parallel system                                        │
     │ - No breaking changes to user workflow                                         │
     │                                                                                │
     │ Phase 2: Metadata-Driven (Optional)                                            │
     │ - Use metadata as source of truth for sync                                     │
     │ - Files remain editable                                                        │
     │ - Gradual transition                                                           │
     │                                                                                │
     │ Phase 3: Full CAS (Optional)                                                   │
     │ - All files in content-addressed storage                                       │
     │ - Symlinks for familiar paths                                                  │
     │ - Maximum deduplication                                                        │
     │                                                                                │
     │ Rollback Plan                                                                  │
     │                                                                                │
     │ If Automerge integration fails:                                                │
     │ 1. Metadata doc is separate from files                                         │
     │ 2. Delete arimalo-metadata.automerge and cas/                                 │
     │ 3. Application continues working with files                                    │
     │ 4. No data loss                                                                │
     │                                                                                │
     │ ---                                                                            │
     │ Critical Files                                                                 │
     │                                                                                │
     │ New Files:                                                                     │
     │ - src-tauri/src/automerge_store.rs - Core Automerge integration                │
     │ - src-tauri/src/sync.rs - Sync protocol implementation                         │
     │                                                                                │
     │ Modified Files:                                                                │
     │ - src-tauri/src/generated_store.rs - Adapt for metadata layer                  │
     │ - src-tauri/src/processing_pipeline.rs - CAS integration                              │
     │ - src-tauri/src/build_cache.rs - Pattern for content addressing                │
     │ - src-tauri/src/main.rs - Add sync commands                                    │
     │ - src/main.ts - Sync and audit trail UI                                        │
     │                                                                                │
     │ Key Dependencies:                                                              │
     │ - automerge = "0.5" - CRDT library (core)                                      │
     │ - autosurgeon = "0.8" - High-level API with derive macros (recommended)        │
     │ - walkdir = "2" - File scanning (may already be present)                       │
     │                                                                                │
     │ Technology Choice: Automerge + Autosurgeon                                     │
     │                                                                                │
     │ Why Automerge:                                                                 │
     │ 1. Audit Trail: Full change history with metadata (critical for financial data)│
     │ 2. File-based Sync: Binary format specification makes Dropbox/Syncthing        │
     │ integration clean                                                              │
     │ 3. Metadata Structures: Maps for accounts/rules, Lists for transaction refs    │
     │ 4. Change Inspection: Access changes for audit logging                         │
     │ 5. Maturity: Production-ready (v3.0) with active maintenance                   │
     │ 6. Performance: v3.0 brought 10x memory reduction                              │
     │                                                                                │
     │ Why Autosurgeon Wrapper:                                                       │
     │ - Derive macros for Reconcile and Hydrate traits                               │
     │ - Serde-like ergonomics for Rust structs                                       │
     │ - Much more idiomatic than raw Automerge API                                   │
     │ - Define metadata types once, auto-generate CRDT operations                    │
     │                                                                                │
     │ Alternative Considered:                                                        │
     │ - Loro: Better performance/docs, but newer (not production-ready yet)          │
     │ - Yrs: Largest ecosystem, but weaker audit trail features                      │
     │ - Decision: Automerge's audit trail + maturity wins for financial use case     │
     │                                                                                │
     │ Example Integration:                                                           │
     │ use automerge::Automerge;                                                      │
     │ use autosurgeon::{Reconcile, Hydrate};                                         │
     │                                                                                │
     │ #[derive(Reconcile, Hydrate, Clone, PartialEq, Debug)]                         │
     │ struct HybridDoc {                                                             │
     │     transaction_refs: Vec<TxnRef>,                                             │
     │     accounts: HashMap<String, AccountMap>,                                     │
     │     rules: HashMap<String, RuleMap>,                                           │
     │     file_manifest: HashMap<String, FileEntry>,                                 │
     │     sync_log: Vec<SyncEvent>,                                                  │
     │ }                                                                              │
     │                                                                                │
     │ #[derive(Reconcile, Hydrate, Clone, PartialEq, Debug)]                         │
     │ struct TxnRef {                                                                │
     │     id: String,                                                                │
     │     content_hash: String,                                                      │
     │     file_path: String,                                                         │
     │     device_origin: String,                                                     │
     │ }                                                                              │
     │                                                                                │
     │ // Create and populate document                                                │
     │ let mut doc = Automerge::new();                                                │
     │ let metadata = HybridDoc { /* ... */ };                                        │
     │ autosurgeon::reconcile(&mut doc, &metadata)?;                                  │
     │                                                                                │
     │ // Save to file (for Dropbox sync)                                             │
     │ let bytes = doc.save();                                                        │
     │ fs::write("arimalo-metadata.automerge", bytes)?;                              │
     │                                                                                │
     │ // Load and merge from another device                                          │
     │ let remote_bytes = fs::read("remote-arimalo-metadata.automerge")?;            │
     │ let remote_doc = Automerge::load(&remote_bytes)?;                              │
     │ doc.merge(&mut remote_doc)?;                                                   │
     │                                                                                │
     │ // Extract merged data                                                         │
     │ let merged: HybridDoc = autosurgeon::hydrate(&doc)?;                           │
     │                                                                                │
     │ ---                                                                            │
     │ Verification Approach                                                          │
     │                                                                                │
     │ Unit Tests                                                                     │
     │                                                                                │
     │ - Automerge document creation and manipulation                                 │
     │ - Content addressing (hash calculation, storage, retrieval)                    │
     │ - Transaction reference generation                                             │
     │ - File manifest building                                                       │
     │                                                                                │
     │ BDD Tests (Cucumber)                                                           │
     │                                                                                │
     │ Feature: Multi-machine sync with Automerge                                     │
     │                                                                                │
     │   Scenario: Sync manual transactions between devices                           │
     │     Given device A adds transaction "Coffee" for $5                            │
     │     And device B adds transaction "Lunch" for $12                              │
     │     When device A syncs with device B                                          │
     │     Then device A has both "Coffee" and "Lunch" transactions                   │
     │     And device B has both "Coffee" and "Lunch" transactions                    │
     │                                                                                │
     │   Scenario: Conflict detection                                                 │
     │     Given both devices edit the same rule                                      │
     │     When they sync                                                             │
     │     Then conflicts are detected and flagged                                    │
     │     And sync log shows both changes                                            │
     │                                                                                │
     │ Integration Tests                                                              │
     │                                                                                │
     │ - Two local directories simulating different devices                           │
     │ - Sync metadata and files between them                                         │
     │ - Verify consistency after sync                                                │
     │ - Test conflict scenarios                                                      │
     │                                                                                │
     │ Manual Testing                                                                 │
     │                                                                                │
     │ - Real-world workflow: laptop and desktop sync                                 │
     │ - Large file handling (multi-MB CSVs, PDFs)                                    │
     │ - Performance testing with thousands of transactions                           │
     │ - UI testing for audit trail and conflict resolution                           │
     │                                                                                │
     │ ---                                                                            │
     │ ---                                                                            │
     │ Proof of Concept: Concrete Implementation Example                              │
     │                                                                                │
     │ 1. Define Metadata Schemas (autosurgeon)                                       │
     │                                                                                │
     │ File: src-tauri/src/automerge_store.rs                                         │
     │                                                                                │
     │ use automerge::Automerge;                                                      │
     │ use autosurgeon::{Reconcile, Hydrate};                                         │
     │ use serde::{Deserialize, Serialize};                                           │
     │ use std::collections::HashMap;                                                 │
     │                                                                                │
     │ #[derive(Reconcile, Hydrate, Clone, PartialEq, Debug, Serialize, Deserialize)] │
     │ pub struct ArimaloMetadata {                                                  │
     │     pub transaction_refs: Vec<TxnRef>,                                         │
     │     pub accounts: HashMap<String, AccountMeta>,                                │
     │     pub rules: HashMap<String, RuleMeta>,                                      │
     │     pub file_manifest: HashMap<String, FileEntry>,                             │
     │     pub sync_log: Vec<SyncEvent>,                                              │
     │     pub devices: HashMap<String, DeviceInfo>,                                  │
     │ }                                                                              │
     │                                                                                │
     │ #[derive(Reconcile, Hydrate, Clone, PartialEq, Debug, Serialize, Deserialize)] │
     │ pub struct TxnRef {                                                            │
     │     pub id: String,              // txn:abc123                                 │
     │     pub content_hash: String,    // SHA-256 of transaction text                │
     │     pub file_path: String,       // sources/manual.transactions                │
     │     pub line_start: usize,       // Line number                                │
     │     pub datetime: String,        // For sorting                                │
     │     pub device_origin: String,   // Which device created it                    │
     │     pub created_at: i64,         // Unix timestamp                             │
     │ }                                                                              │
     │                                                                                │
     │ #[derive(Reconcile, Hydrate, Clone, PartialEq, Debug, Serialize, Deserialize)] │
     │ pub struct AccountMeta {                                                       │
     │     pub path: String,                    // assets:bank:checking               │
     │     pub default_commodity: Option<String>,                                     │
     │     pub opening: Option<String>,         // Opening balance text               │
     │     pub device_origin: String,                                                 │
     │ }                                                                              │
     │                                                                                │
     │ #[derive(Reconcile, Hydrate, Clone, PartialEq, Debug, Serialize, Deserialize)] │
     │ pub struct RuleMeta {                                                          │
     │     pub id: String,                                                            │
     │     pub pattern: String,                                                       │
     │     pub payee: Option<String>,                                                 │
     │     pub contra: Option<String>,                                                │
     │     pub account_folder: String,                                                │
     │     pub device_origin: String,                                                 │
     │ }                                                                              │
     │                                                                                │
     │ #[derive(Reconcile, Hydrate, Clone, PartialEq, Debug, Serialize, Deserialize)] │
     │ pub struct FileEntry {                                                         │
     │     pub content_hash: String,                                                  │
     │     pub relative_path: String,                                                 │
     │     pub file_type: String,       // "csv", "pdf", "transactions"               │
     │     pub size_bytes: u64,                                                       │
     │     pub device_origin: String,                                                 │
     │     pub uploaded_at: i64,                                                      │
     │ }                                                                              │
     │                                                                                │
     │ #[derive(Reconcile, Hydrate, Clone, PartialEq, Debug, Serialize, Deserialize)] │
     │ pub struct SyncEvent {                                                         │
     │     pub timestamp: i64,                                                        │
     │     pub device_id: String,                                                     │
     │     pub event_type: String,      // "txn_added", "file_added", "rule_modified" │
     │     pub target_id: String,       // ID of affected entity                      │
     │     pub details: String,         // JSON details                               │
     │ }                                                                              │
     │                                                                                │
     │ #[derive(Reconcile, Hydrate, Clone, PartialEq, Debug, Serialize, Deserialize)] │
     │ pub struct DeviceInfo {                                                        │
     │     pub device_id: String,                                                     │
     │     pub device_name: String,                                                   │
     │     pub last_seen: i64,                                                        │
     │ }                                                                              │
     │                                                                                │
     │ 2. Initialize Metadata from Existing Files                                     │
     │                                                                                │
     │ use std::path::{Path, PathBuf};                                                │
     │ use std::fs;                                                                   │
     │ use sha2::{Sha256, Digest};                                                    │
     │                                                                                │
     │ pub struct MetadataStore {                                                     │
     │     doc: Automerge,                                                            │
     │     device_id: String,                                                         │
     │     metadata_path: PathBuf,                                                    │
     │ }                                                                              │
     │                                                                                │
     │ impl MetadataStore {                                                           │
     │     pub fn new(metadata_path: PathBuf) -> Result<Self, String> {               │
     │         let device_id = get_device_id()?;                                      │
     │                                                                                │
     │         let doc = if metadata_path.exists() {                                  │
     │             // Load existing metadata                                          │
     │             let bytes = fs::read(&metadata_path)                               │
     │                 .map_err(|e| format!("Failed to read metadata: {}", e))?;      │
     │             Automerge::load(&bytes)                                            │
     │                 .map_err(|e| format!("Failed to load Automerge doc: {}", e))?  │
     │         } else {                                                               │
     │             // Create new metadata                                             │
     │             Automerge::new()                                                   │
     │         };                                                                     │
     │                                                                                │
     │         Ok(Self { doc, device_id, metadata_path })                             │
     │     }                                                                          │
     │                                                                                │
     │     pub fn build_from_sources(&mut self, sources_dir: &Path) -> Result<(),     │
     │ String> {                                                                      │
     │         let mut metadata = ArimaloMetadata {                                  │
     │             transaction_refs: Vec::new(),                                      │
     │             accounts: HashMap::new(),                                          │
     │             rules: HashMap::new(),                                             │
     │             file_manifest: HashMap::new(),                                     │
     │             sync_log: Vec::new(),                                              │
     │             devices: HashMap::new(),                                           │
     │         };                                                                     │
     │                                                                                │
     │         // Scan transaction files                                              │
     │         self.scan_transactions(sources_dir, &mut metadata)?;                   │
     │                                                                                │
     │         // Scan CSV files                                                      │
     │         self.scan_csv_files(sources_dir, &mut metadata)?;                      │
     │                                                                                │
     │         // Scan rules                                                          │
     │         self.scan_rules(sources_dir, &mut metadata)?;                          │
     │                                                                                │
     │         // Add device info                                                     │
     │         metadata.devices.insert(                                               │
     │             self.device_id.clone(),                                            │
     │             DeviceInfo {                                                       │
     │                 device_id: self.device_id.clone(),                             │
     │                 device_name: hostname::get()?.to_string_lossy().to_string(),   │
     │                 last_seen: now(),                                              │
     │             },                                                                 │
     │         );                                                                     │
     │                                                                                │
     │         // Reconcile into Automerge                                            │
     │         autosurgeon::reconcile(&mut self.doc, &metadata)                       │
     │             .map_err(|e| format!("Reconcile failed: {}", e))?;                 │
     │                                                                                │
     │         // Log build event                                                     │
     │         self.log_sync_event("metadata_built", "", "Built from sources")?;      │
     │                                                                                │
     │         Ok(())                                                                 │
     │     }                                                                          │
     │                                                                                │
     │     fn scan_transactions(&self, sources_dir: &Path, metadata: &mut             │
     │ ArimaloMetadata)                                                              │
     │         -> Result<(), String>                                                  │
     │     {                                                                          │
     │         use crate::ledger_parser::parse_transactions_file;                     │
     │                                                                                │
     │         // Parse manual.transactions                                           │
     │         let manual_path = sources_dir.join("manual.transactions");             │
     │         if manual_path.exists() {                                              │
     │             let content = fs::read_to_string(&manual_path)                     │
     │                 .map_err(|e| format!("Read failed: {}", e))?;                  │
     │             let hash = sha256_string(&content);                                │
     │                                                                                │
     │             let parse_result = parse_transactions_file(&content)               │
     │                 .map_err(|e| format!("Parse failed: {}", e))?;                 │
     │                                                                                │
     │             for (idx, txn) in parse_result.transactions.iter().enumerate() {   │
     │                 metadata.transaction_refs.push(TxnRef {                        │
     │                     id: extract_txn_id(&txn.meta).unwrap_or_else(||            │
     │ format!("manual-{}", idx)),                                                    │
     │                     content_hash: sha256_string(&serialize_txn(txn)),          │
     │                     file_path: "sources/manual.transactions".to_string(),      │
     │                     line_start: 0, // TODO: track line numbers                 │
     │                     datetime: txn.datetime.clone(),                            │
     │                     device_origin: self.device_id.clone(),                     │
     │                     created_at: now(),                                         │
     │                 });                                                            │
     │             }                                                                  │
     │                                                                                │
     │             // Add to file manifest                                            │
     │             metadata.file_manifest.insert(                                     │
     │                 hash.clone(),                                                  │
     │                 FileEntry {                                                    │
     │                     content_hash: hash,                                        │
     │                     relative_path: "sources/manual.transactions".to_string(),  │
     │                     file_type: "transactions".to_string(),                     │
     │                     size_bytes: content.len() as u64,                          │
     │                     device_origin: self.device_id.clone(),                     │
     │                     uploaded_at: now(),                                        │
     │                 },                                                             │
     │             );                                                                 │
     │         }                                                                      │
     │                                                                                │
     │         Ok(())                                                                 │
     │     }                                                                          │
     │                                                                                │
     │     fn scan_csv_files(&self, sources_dir: &Path, metadata: &mut                │
     │ ArimaloMetadata)                                                              │
     │         -> Result<(), String>                                                  │
     │     {                                                                          │
     │         use walkdir::WalkDir;                                                  │
     │                                                                                │
     │         for entry in WalkDir::new(sources_dir)                                 │
     │             .follow_links(false)                                               │
     │             .into_iter()                                                       │
     │             .filter_map(|e| e.ok())                                            │
     │         {                                                                      │
     │             if entry.path().extension().and_then(|s| s.to_str()) == Some("csv")│
     │  {                                                                             │
     │                 let content = fs::read(entry.path())                           │
     │                     .map_err(|e| format!("Read CSV failed: {}", e))?;          │
     │                 let hash = sha256_bytes(&content);                             │
     │                                                                                │
     │                 let relative = entry.path()                                    │
     │                     .strip_prefix(sources_dir)                                 │
     │                     .map_err(|e| format!("Path error: {}", e))?;               │
     │                                                                                │
     │                 metadata.file_manifest.insert(                                 │
     │                     hash.clone(),                                              │
     │                     FileEntry {                                                │
     │                         content_hash: hash,                                    │
     │                         relative_path: relative.to_string_lossy().to_string(), │
     │                         file_type: "csv".to_string(),                          │
     │                         size_bytes: content.len() as u64,                      │
     │                         device_origin: self.device_id.clone(),                 │
     │                         uploaded_at: now(),                                    │
     │                     },                                                         │
     │                 );                                                             │
     │             }                                                                  │
     │         }                                                                      │
     │                                                                                │
     │         Ok(())                                                                 │
     │     }                                                                          │
     │                                                                                │
     │     pub fn save(&self) -> Result<(), String> {                                 │
     │         let bytes = self.doc.save();                                           │
     │         fs::write(&self.metadata_path, bytes)                                  │
     │             .map_err(|e| format!("Save failed: {}", e))?;                      │
     │         Ok(())                                                                 │
     │     }                                                                          │
     │                                                                                │
     │     pub fn merge_from_file(&mut self, remote_path: &Path) -> Result<(), String>│
     │  {                                                                             │
     │         let remote_bytes = fs::read(remote_path)                               │
     │             .map_err(|e| format!("Read remote failed: {}", e))?;               │
     │         let mut remote_doc = Automerge::load(&remote_bytes)                    │
     │             .map_err(|e| format!("Load remote failed: {}", e))?;               │
     │                                                                                │
     │         // Merge!                                                              │
     │         self.doc.merge(&mut remote_doc)                                        │
     │             .map_err(|e| format!("Merge failed: {}", e))?;                     │
     │                                                                                │
     │         self.log_sync_event("merged_remote", "", &format!("Merged from {:?}",  │
     │ remote_path))?;                                                                │
     │                                                                                │
     │         Ok(())                                                                 │
     │     }                                                                          │
     │                                                                                │
     │     pub fn get_metadata(&self) -> Result<ArimaloMetadata, String> {           │
     │         autosurgeon::hydrate(&self.doc)                                        │
     │             .map_err(|e| format!("Hydrate failed: {}", e))                     │
     │     }                                                                          │
     │                                                                                │
     │     pub fn log_sync_event(&mut self, event_type: &str, target_id: &str,        │
     │ details: &str)                                                                 │
     │         -> Result<(), String>                                                  │
     │     {                                                                          │
     │         let mut metadata: ArimaloMetadata = autosurgeon::hydrate(&self.doc)   │
     │             .map_err(|e| format!("Hydrate failed: {}", e))?;                   │
     │                                                                                │
     │         metadata.sync_log.push(SyncEvent {                                     │
     │             timestamp: now(),                                                  │
     │             device_id: self.device_id.clone(),                                 │
     │             event_type: event_type.to_string(),                                │
     │             target_id: target_id.to_string(),                                  │
     │             details: details.to_string(),                                      │
     │         });                                                                    │
     │                                                                                │
     │         autosurgeon::reconcile(&mut self.doc, &metadata)                       │
     │             .map_err(|e| format!("Reconcile failed: {}", e))?;                 │
     │                                                                                │
     │         Ok(())                                                                 │
     │     }                                                                          │
     │ }                                                                              │
     │                                                                                │
     │ fn get_device_id() -> Result<String, String> {                                 │
     │     // Use machine ID or generate stable UUID                                  │
     │     machine_uid::get()                                                         │
     │         .map(|id| format!("device-{}", &id[..8]))                              │
     │         .or_else(|_| Ok("device-unknown".to_string()))                         │
     │ }                                                                              │
     │                                                                                │
     │ fn now() -> i64 {                                                              │
     │     std::time::SystemTime::now()                                               │
     │         .duration_since(std::time::UNIX_EPOCH)                                 │
     │         .unwrap()                                                              │
     │         .as_secs() as i64                                                      │
     │ }                                                                              │
     │                                                                                │
     │ fn sha256_string(s: &str) -> String {                                          │
     │     let mut hasher = Sha256::new();                                            │
     │     hasher.update(s.as_bytes());                                               │
     │     format!("{:x}", hasher.finalize())                                         │
     │ }                                                                              │
     │                                                                                │
     │ fn sha256_bytes(b: &[u8]) -> String {                                          │
     │     let mut hasher = Sha256::new();                                            │
     │     hasher.update(b);                                                          │
     │     format!("{:x}", hasher.finalize())                                         │
     │ }                                                                              │
     │                                                                                │
     │ 3. Add Tauri Commands                                                          │
     │                                                                                │
     │ File: src-tauri/src/main.rs                                                    │
     │                                                                                │
     │ use crate::automerge_store::{MetadataStore, ArimaloMetadata};                 │
     │ use std::sync::Mutex;                                                          │
     │                                                                                │
     │ struct AppState {                                                              │
     │     metadata: Mutex<Option<MetadataStore>>,                                    │
     │ }                                                                              │
     │                                                                                │
     │ #[tauri::command]                                                              │
     │ fn init_metadata(sources_dir: String, app_state: tauri::State<AppState>) ->    │
     │ Result<(), String> {                                                           │
     │     let metadata_path =                                                        │
     │ Path::new(&sources_dir).join("arimalo-metadata.automerge");                   │
     │                                                                                │
     │     let mut store = MetadataStore::new(metadata_path)?;                        │
     │     store.build_from_sources(Path::new(&sources_dir))?;                        │
     │     store.save()?;                                                             │
     │                                                                                │
     │     *app_state.metadata.lock().unwrap() = Some(store);                         │
     │                                                                                │
     │     Ok(())                                                                     │
     │ }                                                                              │
     │                                                                                │
     │ #[tauri::command]                                                              │
     │ fn get_sync_log(app_state: tauri::State<AppState>) -> Result<Vec<SyncEvent>,   │
     │ String> {                                                                      │
     │     let metadata_lock = app_state.metadata.lock().unwrap();                    │
     │     let store = metadata_lock.as_ref().ok_or("Metadata not initialized")?;     │
     │                                                                                │
     │     let metadata = store.get_metadata()?;                                      │
     │     Ok(metadata.sync_log)                                                      │
     │ }                                                                              │
     │                                                                                │
     │ #[tauri::command]                                                              │
     │ fn merge_metadata(remote_path: String, app_state: tauri::State<AppState>) ->   │
     │ Result<(), String> {                                                           │
     │     let metadata_lock = app_state.metadata.lock().unwrap();                    │
     │     let mut store = metadata_lock.as_ref().ok_or("Metadata not initialized")?; │
     │                                                                                │
     │     store.merge_from_file(Path::new(&remote_path))?;                           │
     │     store.save()?;                                                             │
     │                                                                                │
     │     Ok(())                                                                     │
     │ }                                                                              │
     │                                                                                │
     │ #[tauri::command]                                                              │
     │ fn list_devices(app_state: tauri::State<AppState>) -> Result<Vec<DeviceInfo>,  │
     │ String> {                                                                      │
     │     let metadata_lock = app_state.metadata.lock().unwrap();                    │
     │     let store = metadata_lock.as_ref().ok_or("Metadata not initialized")?;     │
     │                                                                                │
     │     let metadata = store.get_metadata()?;                                      │
     │     Ok(metadata.devices.values().cloned().collect())                           │
     │ }                                                                              │
     │                                                                                │
     │ fn main() {                                                                    │
     │     tauri::Builder::default()                                                  │
     │         .manage(AppState {                                                     │
     │             metadata: Mutex::new(None),                                        │
     │         })                                                                     │
     │         .invoke_handler(tauri::generate_handler![                              │
     │             init_metadata,                                                     │
     │             get_sync_log,                                                      │
     │             merge_metadata,                                                    │
     │             list_devices,                                                      │
     │             // ... existing commands                                           │
     │         ])                                                                     │
     │         .run(tauri::generate_context!())                                       │
     │         .expect("error while running tauri application");                      │
     │ }                                                                              │
     │                                                                                │
     │ 4. Frontend Integration                                                        │
     │                                                                                │
     │ File: src/main.ts                                                              │
     │                                                                                │
     │ import { invoke } from '@tauri-apps/api/core';                                 │
     │                                                                                │
     │ interface SyncEvent {                                                          │
     │   timestamp: number;                                                           │
     │   device_id: string;                                                           │
     │   event_type: string;                                                          │
     │   target_id: string;                                                           │
     │   details: string;                                                             │
     │ }                                                                              │
     │                                                                                │
     │ interface DeviceInfo {                                                         │
     │   device_id: string;                                                           │
     │   device_name: string;                                                         │
     │   last_seen: number;                                                           │
     │ }                                                                              │
     │                                                                                │
     │ async function initMetadata() {                                                │
     │   const sourcesDir = await resolveSourcesDir();                                │
     │   await invoke('init_metadata', { sourcesDir });                               │
     │ }                                                                              │
     │                                                                                │
     │ async function viewSyncLog() {                                                 │
     │   const events: SyncEvent[] = await invoke('get_sync_log');                    │
     │                                                                                │
     │   // Render timeline                                                           │
     │   const html = events.map(e => `                                               │
     │     <div class="sync-event">                                                   │
     │       <span class="timestamp">${new Date(e.timestamp *                         │
     │ 1000).toLocaleString()}</span>                                                 │
     │       <span class="device">${e.device_id}</span>                               │
     │       <span class="event-type">${e.event_type}</span>                          │
     │       <span class="details">${e.details}</span>                                │
     │     </div>                                                                     │
     │   `).join('');                                                                 │
     │                                                                                │
     │   document.getElementById('sync-log')!.innerHTML = html;                       │
     │ }                                                                              │
     │                                                                                │
     │ async function syncWithRemote(remotePath: string) {                            │
     │   await invoke('merge_metadata', { remotePath });                              │
     │   alert('Metadata merged successfully!');                                      │
     │   await viewSyncLog();                                                         │
     │ }                                                                              │
     │                                                                                │
     │ async function listKnownDevices() {                                            │
     │   const devices: DeviceInfo[] = await invoke('list_devices');                  │
     │                                                                                │
     │   console.log('Known devices:', devices);                                      │
     │   // Render device list UI                                                     │
     │ }                                                                              │
     │                                                                                │
     │ ---                                                                            │
     │ Summary: Technology Stack for Option 2                                         │
     │                                                                                │
     │ Core CRDT:                                                                     │
     │ - automerge = "0.5" - CRDT engine                                              │
     │ - autosurgeon = "0.8" - Ergonomic Rust API                                     │
     │                                                                                │
     │ Why This Stack:                                                                │
     │ 1. Production-ready and actively maintained                                    │
     │ 2. Excellent audit trail (full change history)                                 │
     │ 3. Binary format optimized for file sync                                       │
     │ 4. Derive macros reduce boilerplate                                            │
     │ 5. Strong fit for financial metadata                                           │
     │                                                                                │
     │ Trade-offs Accepted:                                                           │
     │ - Memory overhead (acceptable for metadata-only use)                           │
     │ - Learning curve (mitigated by autosurgeon)                                    │
     │ - Not the absolute fastest (Loro is faster, but less mature)                   │
     │                                                                                │
     │ ---                                                                            │
     │ Implementation Status                                                          │
     │                                                                                │
     │ Decision: Option 2 (Hybrid) confirmed by user.                                │
     │                                                                                │
     │ Phase 1: Metadata Layer — COMPLETE                                             │
     │                                                                                │
     │ Dependencies added (Cargo.toml):                                               │
     │   - automerge = "0.7" (CRDT engine, v0.7.4)                                   │
     │   - autosurgeon = "0.10" (derive macros for Reconcile/Hydrate, v0.10.1)        │
     │   - walkdir = "2" (recursive file scanning)                                    │
     │   - machine-uid = "0.5" (stable device ID)                                     │
     │   - hostname = "0.4" (device name)                                             │
     │                                                                                │
     │ New files:                                                                     │
     │   - src-tauri/src/automerge_store.rs                                           │
     │     Implements MetadataStore with:                                              │
     │     - ArimaloMetadata (transaction_refs, accounts, rules, file_manifest,       │
     │       sync_log, devices)                                                        │
     │     - build_from_sources() — scan transactions, files, and rules               │
     │     - save() / load via MetadataStore::new()                                   │
     │     - merge_from_file() — CRDT merge from remote                               │
     │     - log_sync_event() — audit trail                                           │
     │     - Content hashing (SHA-256) for file manifest                              │
     │                                                                                │
     │   - src-tauri/features/generated/automerge_metadata.feature                    │
     │     6 BDD scenarios covering:                                                   │
     │     - Initialize metadata from sources                                          │
     │     - File manifest with content hashes                                         │
     │     - Save and reload persistence                                               │
     │     - Merge metadata from two devices                                           │
     │     - Sync log events and device tracking                                       │
     │     - Rules tracked in metadata                                                 │
     │                                                                                │
     │ Modified files:                                                                │
     │   - src-tauri/src/lib.rs — registered automerge_store module                   │
     │   - src-tauri/src/main.rs — added Tauri commands:                              │
     │     init_metadata, get_sync_log, merge_metadata, list_devices                  │
     │   - src-tauri/tests/bdd.rs — step definitions for automerge scenarios          │
     │   - src-tauri/Cargo.toml — new dependencies                                    │
     │                                                                                │
     │ Test results:                                                                  │
     │   - 20 unit tests passed (5 new for automerge_store)                           │
     │   - 36 BDD scenarios passed (6 new for automerge metadata)                     │
     │   - All existing tests continue to pass                                        │
     │                                                                                │
     │ Phase 2: Content Addressing — COMPLETE                                         │
     │                                                                                │
     │ New files:                                                                     │
     │   - src-tauri/src/content_store.rs                                             │
     │     Implements ContentStore with:                                               │
     │     - store() / store_file() — hash content (SHA-256) and write to             │
     │       cas/<prefix>/<rest> directory structure                                   │
     │     - retrieve() — get blob by hash                                             │
     │     - verify() — integrity check (Ok / Missing / Corrupted)                    │
     │     - blob_count() — enumerate stored blobs                                     │
     │     - delete_blob() / corrupt_blob() — test helpers                            │
     │     - ingest_sources_to_cas() — scan sources dir and store all files           │
     │     - Automatic deduplication (same hash = skip write)                          │
     │                                                                                │
     │   - src-tauri/features/generated/content_store.feature                         │
     │     7 BDD scenarios covering:                                                   │
     │     - Store and retrieve by hash                                                │
     │     - Deduplication of identical content                                        │
     │     - Different content produces different hashes                               │
     │     - Integrity check passes / detects corruption                              │
     │     - Ingest CSV into CAS with manifest update                                 │
     │     - Missing blob detection during verification                               │
     │                                                                                │
     │ Modified files:                                                                │
     │   - src-tauri/src/lib.rs — registered content_store module                     │
     │   - src-tauri/tests/bdd.rs — CAS step definitions                              │
     │                                                                                │
     │ Test results:                                                                  │
     │   - 26 unit tests passed (6 new for content_store)                             │
     │   - 43 BDD scenarios passed (7 new for CAS)                                    │
     │   - All existing tests continue to pass                                        │
     │                                                                                │
     │ Phase 3: Sync Protocol — COMPLETE                                              │
     │                                                                                │
     │ New files:                                                                     │
     │   - src-tauri/src/sync.rs                                                      │
     │     Implements two-phase sync:                                                  │
     │     - Phase 1: sync_metadata() — merge Automerge docs                          │
     │     - Phase 2: sync_files() — transfer missing CAS blobs by hash               │
     │     - full_sync() — orchestrates both phases                                    │
     │     - diff_manifests() — compare file manifests between devices                │
     │     - SyncResult struct with transfer stats                                     │
     │                                                                                │
     │   - src-tauri/features/generated/sync_protocol.feature                         │
     │     4 BDD scenarios covering:                                                   │
     │     - Full sync between devices with different files                            │
     │     - No transfer when both have same blobs                                     │
     │     - Manifest difference detection                                             │
     │     - Sync timestamp and device awareness                                       │
     │                                                                                │
     │ Modified files:                                                                │
     │   - src-tauri/src/lib.rs — registered sync module                              │
     │   - src-tauri/src/main.rs — added sync_with_remote Tauri command               │
     │   - src-tauri/src/automerge_store.rs — added register_file() method            │
     │   - src-tauri/tests/bdd.rs — sync step definitions                             │
     │                                                                                │
     │ Test results:                                                                  │
     │   - 28 unit tests passed (2 new for sync)                                      │
     │   - 47 BDD scenarios passed (4 new for sync protocol)                          │
     │   - All existing tests continue to pass                                        │
     │                                                                                │
     │ Phase 4: Audit Trail UI — COMPLETE                                              │
     │                                                                                │
     │ Modified files:                                                                │
     │   - src/main.ts — Added:                                                       │
     │     - SyncEvent, DeviceInfo, SyncResponse types                                │
     │     - syncLogOpen, syncLog, devices fields in AppState                         │
     │     - Sidebar "Sync" section with Initialize Metadata and View Sync Log        │
     │     - Known devices display in sidebar                                         │
     │     - Sync log modal with table (Time, Device, Event, Details)                 │
     │     - Export sync log to JSON                                                   │
     │     - Event handlers for init_metadata, get_sync_log, list_devices             │
     │   - src/style.css — Added:                                                     │
     │     - .sidebar__section--sync, .syncDevices styles                             │
     │     - .modal--wide for sync log modal                                          │
     │     - .syncLogTable, .syncLogEvent__type, .syncLogEmpty styles                 │
     │                                                                                │
     │ All 4 phases of Option 2 (Hybrid) are now implemented.                        │
     │                                                                                │
     │ Phase 5: Self-Hosted Relay Server — COMPLETE                                  │
     │                                                                                │
     │ Architecture:                                                                  │
     │   [Device A] <──HTTP──> [Relay Server] <──HTTP──> [Device B]                  │
     │   - Relay binary: arimalo-relay (tiny_http, no async)                        │
     │   - Client: ureq HTTP calls from Tauri commands                               │
     │   - Pairing: 6-digit codes, 10-minute TTL, single-use                        │
     │   - Sync strategy: upload-first (avoids Automerge duplicate-seq errors)       │
     │                                                                                │
     │ New files:                                                                     │
     │   - src-tauri/src/bin/relay.rs — Relay server entry point (CLI)               │
     │   - src-tauri/src/relay/mod.rs — Module declarations                          │
     │   - src-tauri/src/relay/server.rs — HTTP server + URL routing                 │
     │   - src-tauri/src/relay/pairing.rs — Code generation, validation, expiry      │
     │   - src-tauri/src/relay/storage.rs — Per-group metadata + blob storage        │
     │   - src-tauri/src/relay/handlers.rs — Request handlers for all endpoints      │
     │   - src-tauri/src/relay_client.rs — Client HTTP functions (ureq)              │
     │   - features/generated/relay_pairing.feature — 5 pairing scenarios            │
     │   - features/generated/relay_sync.feature — 4 sync scenarios                  │
     │   - features/generated/relay_client.feature — 3 client scenarios              │
     │                                                                                │
     │ Modified files:                                                                │
     │   - src-tauri/Cargo.toml — Added tiny_http, uuid, rand; [[bin]] relay         │
     │   - src-tauri/src/lib.rs — Added pub mod relay, relay_client                  │
     │   - src-tauri/src/main.rs — 5 new Tauri commands (pair, sync, config)         │
     │   - src-tauri/src/automerge_store.rs — metadata_path(), merge_from_bytes()    │
     │   - src-tauri/tests/bdd.rs — 30+ relay step definitions                      │
     │   - src/main.ts — Relay UI (pairing modal, sync button, config)               │
     │   - src/style.css — Pairing code + relay status styles                        │
     │   - .github/workflows/build.yml — Relay binary build + release artifacts      │
     │                                                                                │
     │ API endpoints:                                                                 │
     │   POST /pair/initiate — Create group + pairing code                           │
     │   POST /pair/join — Join group with code                                       │
     │   GET/POST /metadata/{group_id} — Download/upload Automerge metadata          │
     │   GET/POST /blobs/{group_id}/{hash} — Download/upload CAS blobs              │
     │   GET /blobs/{group_id}/list — List remote blob hashes                        │
     │                                                                                │
     │ Test results:                                                                  │
     │   - 36 unit tests passed (8 new for relay modules)                            │
     │   - 59 BDD scenarios passed (12 new for relay)                                │
     │   - All existing tests continue to pass
