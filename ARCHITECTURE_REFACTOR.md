# Architecture Refactor Plan

This document summarizes the current architecture assessment and proposes an incremental refactor plan focused on:

1. Runtime boundary quality (core agent loop vs TUI/CLI)
2. Trait design quality and completeness
3. Extensibility for skills/sub-agents
4. Tool interface evolution (typed IO + per-tool rendering)

## Current State (What Is Good)

- Core LLM domain types are provider-agnostic and centralized (`Role`, `Message`, `ToolCall`, provider request/response types).
- Core loop emits UI-agnostic events via `AgentEvents`.
- TUI and plain CLI rendering are adapters over core events.
- Provider integration is trait-based and testable.
- Tool system already supports per-tool input schemas.

This is a strong foundation and aligns with the intended architecture direction.

## Key Gaps

### 1) Core loop still depends on concrete infrastructure

`AgentLoop` directly depends on:

- `ToolRegistry`
- `PermissionMatcher`
- `SessionStore`

This keeps runtime orchestration coupled to concrete implementations and makes extension/replacement harder than necessary.

### 2) Trait surface is incomplete vs architecture intent

Current core traits cover provider + event sinks, but not:

- tool execution capability boundary
- approval/permission policy boundary
- session persistence boundary

As a result, testing and composition are good for provider/events but less flexible for other runtime edges.

### 3) Extensibility friction for new capabilities

Adding a tool often requires touching multiple locations:

- tool registration
- permission matcher switch logic
- settings schema fields

This increases change surface and creates avoidable coupling.

### 4) Tool output/rendering is not first-class

Tools can define different input formats today (good), but output is currently opaque string data:

- `ToolResult { is_error, output: String }`

Per-tool rendering is currently ad-hoc (example: `todo_write` special handling in TUI). This does not scale.

## Answers To Product Questions

### Is the boundary between core loop and TUI good?

Mostly yes. The event boundary is clean. Main improvement needed: core should depend on traits for tools/permissions/session instead of concrete structs.

### How are the trait definitions?

Provider + event traits are small and good. The trait set is incomplete for long-term architecture goals.

### How is extensibility for skills/sub-agents?

Basic tool extensibility is decent. True skills/sub-agents are not first-class yet (no dedicated domain types/protocol for nested agent work).

### Can different tools have different input/output formats?

- Input: yes, already supported via per-tool JSON schema.
- Output: partially. Different tools can return different string payload formats, but runtime/UI cannot reason over them safely because output is untyped.

### Can different tools specify different rendering styles?

Not in a scalable way today. Rendering customization is currently by ad-hoc UI special cases.

## Refactor Goals

1. Keep core runtime provider-agnostic and UI-agnostic.
2. Make runtime boundaries trait-based and injectable.
3. Reduce multi-file wiring cost for adding capabilities.
4. Make tool output typed and renderer-dispatchable.
5. Introduce explicit support model for skills/sub-agents.

## Proposed Target Design

### A) Core runtime dependency boundaries

Introduce core traits and make `AgentLoop` generic over them:

- `ToolExecutor` (list schemas, execute by name)
- `ApprovalPolicy` (allow/ask/deny + optional approval flow)
- `SessionSink` + `SessionReader` (append/replay event contracts)

Keep existing concrete implementations as adapters:

- `ToolRegistry` -> implements `ToolExecutor`
- `PermissionMatcher` -> implements `ApprovalPolicy`
- `SessionStore` -> implements `SessionSink` + `SessionReader`

### B) Tool descriptor and policy alignment

Add metadata to tool descriptors so permissions are capability-based rather than hardcoded name switches.

Example metadata:

- capability class (`fs_read`, `fs_write`, `network`, `shell`, etc.)
- default risk level
- mutating vs read-only

This lets policy be data-driven and lowers the cost of adding tools.

### C) Typed tool output envelope

Evolve `ToolResult` from opaque string to structured envelope.

Suggested shape:

- `is_error: bool`
- `summary: String` (short preview)
- `content_type: String` (e.g. `text/plain`, `application/json`, `application/vnd.hh.todo+json`)
- `payload: serde_json::Value`

Compatibility path:

- keep legacy `output` during transition
- adapt old tools via shim
- gradually migrate renderers to new fields

### D) Rendering plugin model

Add UI-side renderer registry:

- key by `content_type` (preferred) and optional fallback by tool name
- each renderer maps tool result envelope -> view model

This removes hardcoded tool-specific parsing from `ChatApp` and supports per-tool rendering styles cleanly.

### E) Skills/Sub-agent first-class protocol

Introduce domain entities for nested work:

- `AgentTask` or `SubAgentCall` type with parent correlation ID
- session events for sub-agent lifecycle (start/progress/result)
- explicit limits/policy for recursion/depth/budget

Implement sub-agents as a runtime capability (tool or dedicated trait-backed service), not as UI command hacks.

## Incremental Migration Plan

### Phase 1: Boundary extraction (low risk)

- Add core traits for tools, approvals, and session IO.
- Implement traits on existing structs.
- Refactor `AgentLoop` to consume trait bounds.
- Keep behavior unchanged.

### Phase 2: Tool metadata + policy cleanup

- Extend tool schema/descriptor metadata.
- Replace permission name-switch matching with metadata-driven policy.
- Keep config backward-compatible.

### Phase 3: Typed tool output

- Add structured tool result envelope with compatibility field(s).
- Migrate built-in tools gradually.
- Update session persistence to store envelope cleanly.

### Phase 4: Renderer registry

- Extract tool result rendering from `ChatApp` into renderer modules.
- Add default generic JSON/text renderer.
- Add specialized renderers (todo, diff, table, etc.) by content type.

### Phase 5: Skills/sub-agents

- Add domain/session protocol for nested tasks.
- Add runtime guardrails (depth, step budget, cancellation).
- Integrate into CLI/TUI as passive presentation over events.

## Invariants To Preserve

- No provider-specific wire details in core domain types.
- UI layers remain adapters over core traits/events.
- Session events remain replayable and inspectable.
- Existing commands and user-facing behavior stay stable during early phases.

## Risks And Mitigations

- Risk: breaking backward compatibility in tool outputs.
  - Mitigation: dual-format transition with shims.
- Risk: refactor introduces runtime regressions.
  - Mitigation: add focused integration tests around agent loop/tool approval/session replay.
- Risk: policy behavior changes unexpectedly.
  - Mitigation: golden tests for allow/ask/deny decisions by tool capability.

## Success Criteria

- New tool can be added without editing permission matcher switch statements.
- Different tools can return typed outputs with no UI hardcoding in chat state logic.
- Different rendering styles are selected by `content_type` registry.
- Sub-agent execution has explicit protocol, persistence, and guardrails.
- `AgentLoop` orchestration compiles against trait contracts only.
