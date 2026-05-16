import { describe, expect, test } from "vite-plus/test";
import { estimateLogCostUsd, priceForModel, type ModelPrice } from "./model-pricing.ts";

const gptPrice: ModelPrice = {
  input: 2,
  output: 8,
  cacheRead: 0.5,
  cacheCreation: 1.5,
};

describe("model pricing helpers", () => {
  test("finds prices by direct model key or provider-prefixed tail", () => {
    const prices = new Map<string, ModelPrice>([["gpt-test", gptPrice]]);

    expect(priceForModel(prices, " GPT-TEST ")).toBe(gptPrice);
    expect(priceForModel(prices, "openai/gpt-test")).toBe(gptPrice);
    expect(priceForModel(prices, "missing")).toBeNull();
    expect(priceForModel(prices, null)).toBeNull();
  });

  test("uses existing positive db estimate before local estimate", () => {
    const prices = new Map<string, ModelPrice>([["gpt-test", gptPrice]]);
    expect(
      estimateLogCostUsd(
        {
          estimated_cost_usd: "0.1234",
          upstream_model: "gpt-test",
          requested_model: null,
          input_tokens: 1,
          output_tokens: 1,
          cache_read_tokens: 1,
          cache_creation_tokens: 1,
        } as any,
        prices,
      ),
    ).toBe(0.1234);
  });

  test("estimates token cost and clamps negative token counts", () => {
    const prices = new Map<string, ModelPrice>([["gpt-test", gptPrice]]);
    const cost = estimateLogCostUsd(
      {
        estimated_cost_usd: "0",
        upstream_model: "provider/gpt-test",
        requested_model: null,
        input_tokens: 1_000_000,
        output_tokens: 500_000,
        cache_read_tokens: -100,
        cache_creation_tokens: 2_000_000,
      } as any,
      prices,
    );

    expect(cost).toBe(9);
    expect(
      estimateLogCostUsd(
        {
          estimated_cost_usd: "0",
          upstream_model: "missing",
          requested_model: null,
          input_tokens: 1,
          output_tokens: 1,
          cache_read_tokens: 1,
          cache_creation_tokens: 1,
        } as any,
        prices,
      ),
    ).toBe(0);
  });
});
