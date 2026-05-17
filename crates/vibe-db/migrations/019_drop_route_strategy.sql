-- Phase 3d: forwarding strategy is no longer user-configurable.
-- The gateway now runs a single algorithm: health-bucketed waves (病患先
-- → 健康兜底), with Race / Rotate absorbed as the internal execution
-- engine. `strategy` and `fanout_n` are therefore dropped from `routes`.

ALTER TABLE routes DROP COLUMN strategy;
ALTER TABLE routes DROP COLUMN fanout_n;
