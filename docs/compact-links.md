# Compact Markdown links

Use `/compact-links on` to show Markdown link labels without a trailing destination URL. Click a label with the left mouse button to open its destination. Jcode handles the click, so this does not require terminal OSC 8 hyperlink support or tmux graphics passthrough.

Use `/compact-links off` to restore `label (URL)` display. Labels remain clickable in either mode. `/compact-links status` shows the current setting.

The command applies immediately and saves the preference. Compact links are off by default. The corresponding configuration is:

```toml
[display]
compact_links = true
```

The setting applies to assistant messages, streaming output, and Markdown side panels. Views that do not retain link targets keep the visible URL as a fallback. Bare URLs remain visible. The preference does not change link destinations.
