# Session-first clients

After pairing, connect to the normal authenticated gateway WebSocket. Before
subscribing, send one newline-delimited JSON request (the local socket accepts
the same protocol):

```json
{"type":"list_sessions","id":1}
```

The matching response is a single event, not an Ack/Done sequence:

```json
{"type":"sessions","id":1,"sessions":[{"id":"session_fox","title":"Fix login","working_dir":"/srv/project","is_processing":false}]}
```

`id` is the request's unsigned integer correlation ID. Each entry has a stable
session `id` and a human-readable `title` (session title, friendly name, or ID
fallback). `working_dir` and `is_processing` are optional. Processing status is
best-effort live member status, not a guarantee about a subsequent attachment.
Persisted-only sessions omit processing status. Live metadata takes precedence
where available. Results include live unsaved and persisted sessions, deduplicated
and sorted by ID. Unreadable snapshots are skipped. Enumeration failures return
an ordinary correlated `error` event.

Listing neither creates nor attaches a session, does not invoke a provider, and
keeps the connection open. It can be repeated before or after subscribing. No
new authentication is introduced: the gateway's existing WebSocket authentication
and local socket access controls apply.

Select an existing session on that same connection:

```json
{"type":"subscribe","id":2,"target_session_id":"session_fox"}
```

An omitted working directory is resolved from the existing live session or its
persisted metadata. Unknown targets, or targets without a resolvable directory,
return an error rather than silently creating an unrelated session. Normal
history/bootstrap events and existing takeover rules still apply. To create a
**new** session, omit `target_session_id` and supply an absolute `working_dir`.
Remote continuation retains its existing server-directory validation.

## Compatibility

This is additive for existing clients: no existing payload fields changed and
`sessions` is only sent in response to `list_sessions`. Older servers do not
support this request, so clients should handle an unsupported-request error.
The deliberate safety correction is that an unknown explicit target no longer
falls back to creating a session, even if a working directory was supplied.

## Validation

- Protocol suite: 80 tests passed, including wire shape and optional-field checks.
- `cargo test -p jcode-app-core --lib target_attach_tests`: 9 passed.
- `cargo test -p jcode-app-core --lib lightweight_comm_request_skips_full_session_initialization`: passed. Repeated listing and interleaved ping do not fork a provider, create an agent, or register a client attachment.
- `cargo build --profile selfdev --bin jcode`: passed.
- Isolated binary smoke: HTTP pairing, authenticated WebSocket discovery, repeated
  listing, pre-subscribe ping, target attachment without cwd, post-subscribe
  listing, local socket discovery, and rejection of an unknown target with an
  explicit cwd all passed. Discovery left the persisted snapshot unchanged and
  attachment did not leave an unrelated session. The smoke used a sandboxed
  home/runtime and local Ollama provider with no model messages. No shared-daemon
  restart or installation was performed.
