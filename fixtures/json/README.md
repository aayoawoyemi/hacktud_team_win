# fixtures/json

One JSON example per contract message (WP-2 R2.2). Owned by WP-2.

- Shapes and values come from PRD §6 on the `notes` branch. Values are examples, not targets or decisions.
- `crates/contracts/tests/fixtures.rs` reads every file here, parses it into its Rust type, writes it back,
  and checks that every key and value in the file survives the round trip, then does the same through
  protobuf bytes.
- Port `50070` in `register_request.json` is a placeholder: the agent port is O-17 (open).
- Adding a fixture: add the file here and one line to the `fixtures!` list in `tests/fixtures.rs`.
