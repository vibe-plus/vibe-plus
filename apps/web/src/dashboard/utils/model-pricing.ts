import type { RequestLog } from "../api/client.ts";

export type ModelPrice = {
  input: number;
  output: number;
  cacheRead: number;
  cacheCreation: number;
};

type ModelsDevModel = {
  id?: string;
  name?: string;
  cost?: {
    input?: number;
    output?: number;
    cache_read?: number;
    cache_creation?: number;
    cacheRead?: number;
    cacheCreation?: number;
  };
};

type ModelsDevProvider = {
  models?: Record<string, ModelsDevModel>;
};

let priceCache: Promise<Map<string, ModelPrice>> | null = null;

function normModelKey(model: string): string {
  return model.trim().toLowerCase();
}

function addPrice(out: Map<string, ModelPrice>, key: string | undefined, price: ModelPrice) {
  if (!key?.trim()) return;
  const normalized = normModelKey(key);
  if (!normalized || out.has(normalized)) return;
  out.set(normalized, price);
}

export function loadModelPrices(): Promise<Map<string, ModelPrice>> {
  if (priceCache) return priceCache;
  priceCache = fetch("https://models.dev/api.json")
    .then((res) => (res.ok ? res.json() : Promise.reject(new Error(`models.dev ${res.status}`))))
    .then((raw: Record<string, ModelsDevProvider>) => {
      const out = new Map<string, ModelPrice>();
      for (const provider of Object.values(raw)) {
        for (const [modelKey, model] of Object.entries(provider.models ?? {})) {
          const cost = model.cost;
          if (!cost) continue;
          const price: ModelPrice = {
            input: Number(cost.input ?? 0),
            output: Number(cost.output ?? 0),
            cacheRead: Number(cost.cache_read ?? cost.cacheRead ?? 0),
            cacheCreation: Number(cost.cache_creation ?? cost.cacheCreation ?? 0),
          };
          addPrice(out, modelKey, price);
          addPrice(out, model.id, price);
          addPrice(out, model.name, price);
        }
      }
      return out;
    })
    .catch(() => new Map<string, ModelPrice>());
  return priceCache;
}

export function priceForModel(
  prices: Map<string, ModelPrice>,
  model: string | null | undefined,
): ModelPrice | null {
  if (!model?.trim()) return null;
  const direct = prices.get(normModelKey(model));
  if (direct) return direct;
  const tail = model.split("/").pop();
  if (tail && tail !== model) return prices.get(normModelKey(tail)) ?? null;
  return null;
}

export function estimateLogCostUsd(log: RequestLog, prices: Map<string, ModelPrice>): number {
  const existing = Number(log.estimated_cost_usd);
  if (Number.isFinite(existing) && existing > 0) return existing;
  const price = priceForModel(prices, log.upstream_model ?? log.requested_model);
  if (!price) return 0;
  return (
    (Math.max(0, log.input_tokens) * price.input +
      Math.max(0, log.output_tokens) * price.output +
      Math.max(0, log.cache_read_tokens) * price.cacheRead +
      Math.max(0, log.cache_creation_tokens) * price.cacheCreation) /
    1_000_000
  );
}
