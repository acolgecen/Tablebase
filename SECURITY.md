# Tablebase security model

Tablebase treats SQL entered in its editor as trusted local-user input. The SQL
guardrail accepts exactly one parsed read-only query, and cached source databases
are attached to query connections with `READ_ONLY`.

This guarantee protects imported source data from SQL mutation. It is not an OS
sandbox for the SQL author: DuckDB table functions available in a query may read
other files that the current macOS user can access. Changing that policy would
remove existing DuckDB functionality, so external reads remain enabled.

The webview uses a restrictive content security policy, exposes no global Tauri
API bundle, and has only the core capability needed to invoke Tablebase's own
commands. Native file dialogs and filesystem operations remain in Rust.

Please report security issues privately to the repository owner rather than in a
public issue.
