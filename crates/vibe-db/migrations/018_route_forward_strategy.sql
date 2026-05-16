-- Phase 3: route-level forwarding strategy.
-- `rotate` (default) keeps today's round-robin / circuit-aware sequential behavior.
-- `race` fans out the first `fanout_n` credentials concurrently — winner = first
-- upstream to emit 200 + first body byte; losers get aborted.
-- `fallback` is a strict sequential mode (no rotation), useful for cost-sensitive routes.

ALTER TABLE routes ADD COLUMN strategy TEXT NOT NULL DEFAULT 'rotate';
ALTER TABLE routes ADD COLUMN fanout_n  INTEGER NOT NULL DEFAULT 2;
