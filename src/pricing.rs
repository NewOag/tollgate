//! Computes a request's USD cost from its token usage and a configured
//! price table. Cost is computed once, at write time, using whatever
//! price was in effect then — see [`crate::config::ModelPrice`].

use std::collections::HashMap;

use crate::config::ModelPrice;
use crate::format::Usage;

pub type Table = HashMap<String, ModelPrice>;

const TOKENS_PER_MILLION: f64 = 1_000_000.0;

/// Computes USD cost for one request's usage under `table`. Returns 0 if
/// `model` has no entry — unpriced models simply aren't tracked, not an
/// error.
pub fn cost_usd(model: &str, usage: &Usage, table: &Table) -> f64 {
    let Some(price) = table.get(model) else {
        return 0.0;
    };

    let plain_input = (usage.prompt_tokens - usage.cache_creation_tokens - usage.cache_read_tokens).max(0);
    let cached_input_rate = fallback(price.cached_input_per_mtok, price.input_per_mtok);
    let cache_write_rate = fallback(price.cache_write_per_mtok, price.input_per_mtok);

    tokens_cost(plain_input, price.input_per_mtok)
        + tokens_cost(usage.cache_read_tokens, cached_input_rate)
        + tokens_cost(usage.cache_creation_tokens, cache_write_rate)
        + tokens_cost(usage.completion_tokens, price.output_per_mtok)
}

/// A zero cache rate means "not priced separately" — fall back to the
/// plain input rate.
fn fallback(rate: f64, default: f64) -> f64 {
    if rate == 0.0 {
        default
    } else {
        rate
    }
}

fn tokens_cost(tokens: i64, rate_per_mtok: f64) -> f64 {
    (tokens as f64 / TOKENS_PER_MILLION) * rate_per_mtok
}

#[cfg(test)]
mod tests {
    use super::*;

    fn price(input: f64, output: f64, cached_input: f64, cache_write: f64) -> ModelPrice {
        ModelPrice {
            input_per_mtok: input,
            output_per_mtok: output,
            cached_input_per_mtok: cached_input,
            cache_write_per_mtok: cache_write,
        }
    }

    fn table(model: &str, p: ModelPrice) -> Table {
        HashMap::from([(model.to_string(), p)])
    }

    #[test]
    fn pure_input_only() {
        let usage = Usage {
            prompt_tokens: 1_000_000,
            ..Default::default()
        };
        let t = table("gpt-4o", price(2.0, 8.0, 0.0, 0.0));
        assert_eq!(cost_usd("gpt-4o", &usage, &t), 2.0);
    }

    #[test]
    fn pure_output_only() {
        let usage = Usage {
            completion_tokens: 1_000_000,
            ..Default::default()
        };
        let t = table("gpt-4o", price(2.0, 8.0, 0.0, 0.0));
        assert_eq!(cost_usd("gpt-4o", &usage, &t), 8.0);
    }

    #[test]
    fn cache_read_uses_cached_rate() {
        let usage = Usage {
            prompt_tokens: 1_000_000,
            cache_read_tokens: 1_000_000,
            ..Default::default()
        };
        // plain_input = prompt_tokens - cache_read = 0, so only the
        // cache_read leg contributes.
        let t = table("gpt-4o", price(2.0, 8.0, 0.5, 0.0));
        assert_eq!(cost_usd("gpt-4o", &usage, &t), 0.5);
    }

    #[test]
    fn cache_write_uses_cache_write_rate() {
        let usage = Usage {
            prompt_tokens: 1_000_000,
            cache_creation_tokens: 1_000_000,
            ..Default::default()
        };
        let t = table("claude-3-5-sonnet", price(3.0, 15.0, 0.3, 3.75));
        assert_eq!(cost_usd("claude-3-5-sonnet", &usage, &t), 3.75);
    }

    #[test]
    fn cache_rates_fall_back_to_input_rate_when_unset() {
        let usage = Usage {
            prompt_tokens: 1_000_000,
            cache_read_tokens: 1_000_000,
            ..Default::default()
        };
        let t = table("model", price(4.0, 10.0, 0.0, 0.0));
        assert_eq!(cost_usd("model", &usage, &t), 4.0);
    }

    #[test]
    fn unpriced_model_costs_zero() {
        let usage = Usage {
            prompt_tokens: 1_000_000,
            completion_tokens: 1_000_000,
            ..Default::default()
        };
        let t: Table = HashMap::new();
        assert_eq!(cost_usd("unknown-model", &usage, &t), 0.0);
    }

    #[test]
    fn prompt_tokens_equal_to_cache_sum_yields_zero_plain_input() {
        let usage = Usage {
            prompt_tokens: 100,
            cache_creation_tokens: 40,
            cache_read_tokens: 60,
            ..Default::default()
        };
        let t = table("model", price(2.0, 8.0, 0.5, 1.0));
        // plain_input = 100 - 40 - 60 = 0.
        let want = tokens_cost(60, 0.5) + tokens_cost(40, 1.0);
        assert_eq!(cost_usd("model", &usage, &t), want);
    }

    #[test]
    fn combined_input_output_and_cache() {
        let usage = Usage {
            prompt_tokens: 2_000_000,
            completion_tokens: 500_000,
            cache_creation_tokens: 200_000,
            cache_read_tokens: 300_000,
            total_tokens: 2_500_000,
        };
        let t = table("model", price(3.0, 15.0, 0.3, 3.75));
        // plain_input = 2_000_000 - 200_000 - 300_000 = 1_500_000
        let want = tokens_cost(1_500_000, 3.0)
            + tokens_cost(300_000, 0.3)
            + tokens_cost(200_000, 3.75)
            + tokens_cost(500_000, 15.0);
        assert_eq!(cost_usd("model", &usage, &t), want);
    }
}
