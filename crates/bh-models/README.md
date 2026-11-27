# bh-models

The model catalog compiled into the client: the hand-edited catalog files,
strict schemas, and transport pinning.

## Scope

- Catalog data: `catalog/models.json` and `catalog/mcp-services.json`,
  embedded with `include_str!` and edited by hand. The client never fetches
  a catalog over the network.
- Resolution: the embedded catalog, pinned against the compiled transports;
  if it fails to parse, the compiled fallback models
  (`bh-ai` `models_generated`) are used.
- Strict schemas: serde `deny_unknown_fields`, version gate (models
  `schemaVersion == 1`), invalid entries skipped, compat validation per
  `api`.
- Transport pinning: every catalog model must match a compiled
  `(provider, api, baseUrl)` tuple; catalog data never carries request
  headers.

## Non-goals

- The plugins/MCP catalog serde and resolution (`bh-core`; this crate only
  hands it the embedded asset).
- Session auth storage, `models.json` parsing, and registry merges
  (`bh-core`): the user's local `models.json` takes precedence over this
  catalog and is merged by its owner.

## Public API

- `ModelCatalog` (`default()`, `from_document()`, `models()`)
- `catalog::{MODEL_CATALOG, MCP_SERVICE_CATALOG, load_catalog_models}`
- `schema::{parse_model_catalog, InvalidEntries, ModelCatalogV1}`
- `compat::is_model_compat`
- `pinning::{parse_provider_model_catalog, pin_catalog_models, PinnedTemplates}`
- `transports::{compiled_models, compiled_transport_tuples}`

## Dependencies (direction compliance)

Depends on `bh-ai` (compiled transport templates: `models_generated`) and
`bh-types` (shared `Model`). Nothing else in the workspace. Consumers live
at or above `bh-agent` (registry wiring in `bh-core`).
