# Application guidance

This guide applies to `src/`. It supplements the [root guidance](../AGENTS.md)
for the CLI and TUI.
Interaction/rendering regressions live in the root `tests/` directory; use the
same behavior requirements when updating those tests.

## Responsibilities

- `main.rs`: minimal entry point; `cli/`: arguments, dispatch, and bounded input.
- `auth.rs` and `json.rs`: terminal authentication and CLI output.
- `logging.rs` and `logging/`: optional private diagnostics, filtering, and bounded file writing.
- `tui.rs` and `tui/`: terminal lifecycle, events, and bounded backend workers.
- `app.rs` and `app/`: state, selection, filtering, and typed dialogs/effects.
- `ui.rs` and `ui/`: cached-state rendering, themes, viewports, and pointer geometry.
- `presentation.rs`: application-specific traffic formatting; text sanitization is core-owned.

## Interaction and architecture

- Import core data/validation and typed macOS operations directly. Do not expose
  backend internals or implement another collection/persistence path in the frontend.
- Rendering and input handlers must not query filesystems, native APIs, or subprocesses.
  Coordinate system work through bounded workers; keep the CLI orchestration explicit.
- Reuse the same typed confirmation/authentication flow for keyboard and mouse actions.
  Confirm the complete configuration proposal or exact captured process list and signal.
- Suspend raw mode and mouse capture for normal sudo authentication. Restore terminal
  state on errors and shutdown; never collect passwords or let workers prompt.
- Keep queues bounded and coalesce observation interest/refresh requests. Accepted
  operations take precedence over sampling; quitting waits for authorized PF transactions.
- Keep diagnostics separate from terminal output. Use explicit static categories,
  counts, durations, and outcomes; never log paths, endpoints, configuration, raw
  arguments/output, process metadata, or error display. Follow [the event map](../docs/logging.md).
- Keep selection stable as observations reorder or disappear. Freeze activity, resources,
  and captured evidence together while continuing to update current firewall controls.
- Drive resource collection from actual visibility, inspection, and sorting interest.
  Hiding resources or freezing Activity stops native counters, not the `nettop` backend.
- Keep uncertainty visible: CPU is per-core, memory is estimated, partial/stale values
  are marked, and observed traffic never supplies an enforcement verdict.

## Rendering and validation

- Sanitize untrusted text with core helpers at output boundaries. Keep exact paths,
  identities, rule IDs, and mutation targets in state; never substitute display labels.
- Pointer targets follow rendered geometry, including table offsets and clipping.
  Active dialogs block underlying targets; scrolling, focus, and keyboard/mouse
  behavior must agree, including narrow terminals.
- Test selection, filtering, confirmation/cancellation, worker completion, and
  mouse geometry using cached fixtures, without real firewall mutation.
- For UI changes, export actual cells from the repository root:

  ```sh
  cargo test -p rooklet --locked --test visual_preview -- --ignored
  ```

  Use `scripts/render-preview.py` (requires Pillow) to render the JSON under
  `target/visual-preview/`, then inspect multiple sizes, including 50 × 17 and
  80 × 24 layouts. Generated previews stay under ignored `target/`.
- Follow the root workspace validation and keep user-facing help/documentation current.
