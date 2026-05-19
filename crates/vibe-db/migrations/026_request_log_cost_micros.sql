-- Align request_logs with dashboard aggregates that sum integer micro-USD.
ALTER TABLE request_logs ADD COLUMN estimated_cost_usd_micros INTEGER NOT NULL DEFAULT 0;

-- Backfill from legacy TEXT column (micro-USD, 6 decimal places).
UPDATE request_logs
SET estimated_cost_usd_micros = CAST(ROUND(CAST(estimated_cost_usd AS REAL) * 1000000) AS INTEGER)
WHERE estimated_cost_usd_micros = 0
  AND estimated_cost_usd IS NOT NULL
  AND TRIM(estimated_cost_usd) != ''
  AND TRIM(estimated_cost_usd) != '0';
