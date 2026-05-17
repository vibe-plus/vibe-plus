-- Phase A: drop the routes table entirely.
--
-- Per-model routing (Sonnet→provider, haiku→background, think→model, …)
-- was an over-designed config surface. The current scenario is "Codex App
-- talking to Codex Models" — direct, no custom routing — and any future
-- multi-vendor routing will not come through a user-editable config.
-- Provider matching now flows entirely through `router::candidates`, which
-- selects on `provider.kind` + `model_aliases` alone.

DROP TABLE IF EXISTS routes;
