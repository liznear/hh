# Protocol Normalization Plan

## Background

We observed leaked `</think>` tokens in assistant-visible output.
Current behavior in `src/provider/openai_compatible.rs` already consumes native reasoning fields (`reasoning`, `thinking`, `reasoning_content`) into the thinking channel, but assistant `content` is still forwarded verbatim.
If upstream providers leak reasoning delimiters in `content`, those delimiters are rendered and persisted.

This indicates a protocol contract gap, not just a rendering bug.

## Rationale

### Why this is not a UI bug
- The leak is present in persisted session events (`message` content), not only in display.
- TUI and CLI renderers correctly display what they receive.
- Fixing only UI would leave corrupted session history, replay output, and downstream behavior.

### Why this is not solved by native reasoning fields alone
- Native reasoning fields reduce dependence on tags, but do not guarantee `content` cleanliness.
- Some providers emit mixed payloads:
  - reasoning in structured fields
  - plus tagged or stray delimiters in `content`
- Without normalization, assistant channel remains vulnerable.

### Architectural principle
Normalize at the provider boundary so the runtime can rely on a stable invariant:
- `assistant.content` is user-visible answer text only.
- `thinking` contains reasoning text only.
- Reasoning delimiters do not cross into assistant content.

This keeps core loop, persistence, and UI provider-agnostic and consistent.

## Goals

1. Enforce channel separation invariants in provider adapter.
2. Prevent reasoning delimiter leakage into assistant content.
3. Support both streaming and non-streaming responses.
4. Handle chunk-split tag boundaries robustly.
5. Preserve compatibility with providers that only use native reasoning fields.
6. Avoid duplicate reasoning emission when both native fields and tags are present.

## Non-goals

- Building a full XML parser.
- Reformatting or post-processing natural assistant content beyond reasoning-delimiter normalization.
- Provider-specific UI behaviors.

## Invariants to Enforce

1. Assistant channel (`AssistantDelta` / `assistant_message.content`) contains no control delimiters: `<think>`, `</think>`, `<thinking>`, `</thinking>`.
2. Thinking channel aggregates:
   - native reasoning fields (`reasoning`, `thinking`, `reasoning_content`)
   - optional extracted tag-body reasoning from `content` when configured/needed.
3. Delimiter fragments split across stream chunks must not leak.
4. Session persistence receives already-normalized data.

## Normalization Policy

### Tag set
Initial supported tag pairs:
- `<think>` ... `</think>`
- `<thinking>` ... `</thinking>`

### Precedence / dedupe policy
- Native reasoning fields are authoritative.
- If native reasoning appears in a chunk/response and a tagged reasoning block is also found in content:
  - prefer native reasoning for thinking channel;
  - strip tagged delimiters from assistant channel;
  - avoid double-appending equivalent tagged reasoning text unless we explicitly choose merge mode.
- If no native reasoning exists, extract tag-body text into thinking channel.

### Stray delimiter handling
- Remove standalone opening/closing reasoning tags from assistant channel.
- Do not remove arbitrary angle-bracket text that is not one of the supported reasoning tags.

## Detailed Implementation Plan

### 1) Add protocol normalizer module (provider layer)
Create a focused internal normalizer in `src/provider/openai_compatible.rs` or a sibling module (e.g., `src/provider/reasoning_normalizer.rs`) with:

- A small state struct for streaming:
  - `inside_reasoning_block: bool`
  - `carry: String` (for partial token boundaries)
- Pure function for non-streaming whole-string normalization.
- Streaming function:
  - input: raw assistant `delta.content`, boolean/native reasoning presence
  - output:
    - `assistant_visible_delta`
    - `thinking_extracted_delta`
  - consumes split tokens safely.

### 2) Integrate into streaming path
In `apply_stream_chunk` (`src/provider/openai_compatible.rs`):
- Process native thinking fields first, track `native_reasoning_seen`.
- Pass `delta.content` through streaming normalizer.
- Emit:
  - `AssistantDelta(normalized_visible)`
  - `ThinkingDelta(extracted_reasoning)` only when policy permits and non-empty.
- Ensure the aggregate `assistant` and `thinking` buffers use normalized values, not raw values.

### 3) Integrate into non-streaming path
In `parse_chat_response`:
- Normalize `message.content` before assigning `assistant_message.content`.
- Merge extracted tagged reasoning into `thinking` only when policy allows and native reasoning absence/dedupe conditions are met.

### 4) Keep agent loop unchanged
`src/core/agent/mod.rs` should remain unchanged. It already handles distinct channels correctly once provider output is normalized.

### 5) Add tests (provider-level)
Add focused tests in provider test files (new or existing):

1. **Trailing close tag**
   - input content: `"...done</think>"`
   - expected assistant: `"...done"`
   - expected thinking: unchanged/empty unless extractable block exists.

2. **Inline tagged block**
   - input content: `"<think>hidden steps</think>Final answer"`
   - expected assistant: `"Final answer"`
   - expected thinking: `"hidden steps"` (if no native reasoning).

3. **Streaming split delimiter**
   - chunks: `"</thi"` + `"nk>"` (or split open/close tags)
   - expected: no delimiter leakage.

4. **Native + tagged mixed**
   - native reasoning present and content includes `<think>...</think>`
   - expected: assistant stripped, thinking deduped per policy.

5. **Non-reasoning angle bracket text**
   - content with `<tag>` not in supported tag set
   - expected: preserved.

### 6) Optional telemetry/debug signal
Add debug-only counters/logs (if project conventions allow) for:
- stripped delimiter count
- extracted tagged reasoning count
This helps validate behavior against real provider streams.

## Rollout / Safety Strategy

1. Implement behind deterministic default behavior (no runtime flag required initially).
2. Run:
   - `cargo check`
   - `cargo test`
   - targeted provider tests
3. Validate with a captured problematic session replay and confirm no new `</think>` persistence.
4. If risk concerns remain, gate extraction-vs-strip behavior behind a config toggle after baseline fix.

## Risks and Mitigations

- **Risk:** Over-stripping legitimate user text containing `<think>` literals.
  - **Mitigation:** strip only exact supported reasoning tags; preserve unknown tags.
- **Risk:** Duplicate reasoning when native and tagged reasoning coexist.
  - **Mitigation:** native-first policy + dedupe guard.
- **Risk:** Streaming boundary bugs.
  - **Mitigation:** explicit state machine tests for split chunks and EOF flush behavior.

## Acceptance Criteria

1. No `</think>` or `<think>` tokens appear in assistant-visible output for known leak cases.
2. Existing native reasoning behavior remains functional.
3. Session message events store normalized assistant content.
4. Provider tests cover streaming and non-streaming normalization cases.
5. No regressions in existing TUI thinking rendering tests.

## Open Decisions (to confirm before implementation)

1. Should extracted tag-body reasoning be appended when native reasoning also exists (`merge`) or dropped (`native-only`)?
   - Recommended default: `native-only` to avoid duplication.
2. Should `<thinking>` tags be supported immediately alongside `<think>`?
   - Recommended default: yes, both.
3. Should we add a user-facing config switch now or only if needed?
   - Recommended default: no switch initially; keep policy internal and deterministic.
