//! Example 04 -- Multi-Model Team with Custom Tools
//!
//! Demonstrates:
//! - Mixing Anthropic and OpenAI models in the same team
//! - Defining custom tools implementing the ToolDefinition trait
//! - Building agents with a custom ToolRegistry
//! - Running a team goal that uses the custom tools
//!
//! Run:
//!   cargo run --example multi_model_team
//!
//! Prerequisites:
//!   ANTHROPIC_API_KEY and optionally OPENAI_API_KEY env vars must be set.

use async_trait::async_trait;
use open_multi_agent::prelude::*;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Custom tools -- implemented via ToolDefinition trait
// ---------------------------------------------------------------------------

/// A custom tool that fetches live exchange rates from a public API.
struct ExchangeRateTool;

#[async_trait]
impl ToolDefinition for ExchangeRateTool {
    fn name(&self) -> &str {
        "get_exchange_rate"
    }

    fn description(&self) -> &str {
        "Get the current exchange rate between two currencies. \
         Returns the rate as a decimal: 1 unit of `from` = N units of `to`."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "from": { "type": "string", "description": "ISO 4217 currency code, e.g. \"USD\"" },
                "to": { "type": "string", "description": "ISO 4217 currency code, e.g. \"EUR\"" }
            },
            "required": ["from", "to"]
        })
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _context: &ToolUseContext,
    ) -> ToolResult {
        let from = input["from"].as_str().unwrap_or("USD");
        let to = input["to"].as_str().unwrap_or("EUR");

        let url = format!(
            "https://api.exchangerate.host/convert?from={}&to={}&amount=1",
            from, to
        );

        match reqwest::get(&url).await {
            Ok(resp) => {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    let rate = json["result"]
                        .as_f64()
                        .or_else(|| json["info"]["rate"].as_f64());

                    if let Some(rate) = rate {
                        return ToolResult {
                            data: serde_json::json!({
                                "from": from,
                                "to": to,
                                "rate": rate,
                                "timestamp": chrono::Utc::now().to_rfc3339()
                            })
                            .to_string(),
                            is_error: false,
                        };
                    }
                }
                // Fallback stub
                let stub = 0.8 + (rand_stub() * 0.5);
                ToolResult {
                    data: serde_json::json!({
                        "from": from, "to": to, "rate": stub,
                        "note": "Live fetch failed. Using stub rate."
                    })
                    .to_string(),
                    is_error: false,
                }
            }
            Err(e) => {
                let stub = 0.8 + (rand_stub() * 0.5);
                ToolResult {
                    data: serde_json::json!({
                        "from": from, "to": to, "rate": stub,
                        "note": format!("Live fetch failed ({}). Using stub rate.", e)
                    })
                    .to_string(),
                    is_error: false,
                }
            }
        }
    }
}

/// Simple deterministic "random" for stub rates (no rand crate dependency).
fn rand_stub() -> f64 {
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    (nanos % 10000) as f64 / 10000.0
}

/// A custom tool that formats a number as a localised currency string.
struct FormatCurrencyTool;

#[async_trait]
impl ToolDefinition for FormatCurrencyTool {
    fn name(&self) -> &str {
        "format_currency"
    }

    fn description(&self) -> &str {
        "Format a number as a currency string."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "amount": { "type": "number", "description": "The numeric amount to format." },
                "currency": { "type": "string", "description": "ISO 4217 currency code, e.g. \"USD\"." }
            },
            "required": ["amount", "currency"]
        })
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _context: &ToolUseContext,
    ) -> ToolResult {
        let amount = input["amount"].as_f64().unwrap_or(0.0);
        let currency = input["currency"].as_str().unwrap_or("USD");
        ToolResult {
            data: format!("{:.2} {}", amount, currency),
            is_error: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: build an Agent with both built-in and custom tools
// ---------------------------------------------------------------------------

fn build_custom_agent(
    config: AgentConfig,
    extra_tools: Vec<Arc<dyn ToolDefinition>>,
) -> Arc<Agent> {
    let mut registry = ToolRegistry::new();
    register_built_in_tools(&mut registry);
    for tool in extra_tools {
        registry.register(tool);
    }
    let registry = Arc::new(registry);
    let executor = Arc::new(ToolExecutor::new(registry.clone(), None));
    Arc::new(Agent::new(config, registry, executor))
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let use_openai = std::env::var("OPENAI_API_KEY").is_ok();

    let researcher_config = AgentConfig {
        name: "researcher".to_string(),
        model: "claude-sonnet-4-6".to_string(),
        provider: Some(Provider::Anthropic),
        system_prompt: Some(
            "You are a financial data researcher.\n\
             Use the get_exchange_rate tool to fetch current rates between the currency pairs you are given.\n\
             Return the raw rates as a JSON object keyed by pair, e.g. { \"USD/EUR\": 0.91, \"USD/GBP\": 0.79 }."
                .to_string(),
        ),
        tools: vec!["get_exchange_rate".to_string()],
        max_turns: Some(6),
        temperature: Some(0.0),
        ..Default::default()
    };

    let analyst_config = AgentConfig {
        name: "analyst".to_string(),
        model: if use_openai {
            "gpt-4o".to_string()
        } else {
            "claude-sonnet-4-6".to_string()
        },
        provider: Some(if use_openai {
            Provider::OpenAI
        } else {
            Provider::Anthropic
        }),
        system_prompt: Some(
            "You are a foreign exchange analyst.\n\
             You receive exchange rate data and produce a short briefing.\n\
             Use format_currency to show example conversions.\n\
             Keep the briefing under 200 words."
                .to_string(),
        ),
        tools: vec!["format_currency".to_string()],
        max_turns: Some(4),
        temperature: Some(0.3),
        ..Default::default()
    };

    // Build agents with custom tools
    let researcher = build_custom_agent(
        researcher_config,
        vec![Arc::new(ExchangeRateTool)],
    );
    let analyst = build_custom_agent(
        analyst_config,
        vec![Arc::new(FormatCurrencyTool)],
    );

    // -------------------------------------------------------------------------
    // Run with AgentPool
    // -------------------------------------------------------------------------

    println!("Multi-model team with custom tools");
    println!(
        "Providers: researcher=anthropic, analyst={}",
        if use_openai { "openai (gpt-4o)" } else { "anthropic (fallback)" }
    );
    println!("Custom tools: get_exchange_rate, format_currency\n");

    let mut pool = AgentPool::new(1);
    pool.add(researcher);
    pool.add(analyst);

    // Step 1: researcher fetches the rates
    println!("[1/2] Researcher fetching FX rates...");
    let research_result = pool
        .run(
            "researcher",
            "Fetch exchange rates for these pairs using the get_exchange_rate tool:\n\
             - USD to EUR\n\
             - USD to GBP\n\
             - USD to JPY\n\
             - EUR to GBP\n\n\
             Return the results as a JSON object: { \"USD/EUR\": <rate>, \"USD/GBP\": <rate>, ... }",
        )
        .await
        .expect("Pool run failed");

    if !research_result.success {
        eprintln!("Researcher failed: {}", research_result.output);
        std::process::exit(1);
    }

    let tool_names: Vec<&str> = research_result
        .tool_calls
        .iter()
        .map(|c| c.tool_name.as_str())
        .collect();
    println!("Researcher done. Tool calls made: {}", tool_names.join(", "));

    // Step 2: analyst writes the briefing
    println!("\n[2/2] Analyst writing FX briefing...");
    let analyst_prompt = format!(
        "Here are the current FX rates gathered by the research team:\n\n\
         {}\n\n\
         Using format_currency, show what $1,000 USD and EUR 1,000 convert to in each of the other currencies.\n\
         Then write a short FX market briefing (under 200 words) covering:\n\
         - Each rate with a brief observation\n\
         - The strongest and weakest currency in the set\n\
         - One-sentence market comment",
        research_result.output
    );

    let analyst_result = pool
        .run("analyst", &analyst_prompt)
        .await
        .expect("Pool run failed");

    if !analyst_result.success {
        eprintln!("Analyst failed: {}", analyst_result.output);
        std::process::exit(1);
    }

    let tool_names: Vec<&str> = analyst_result
        .tool_calls
        .iter()
        .map(|c| c.tool_name.as_str())
        .collect();
    println!("Analyst done. Tool calls made: {}", tool_names.join(", "));

    // -------------------------------------------------------------------------
    // Results
    // -------------------------------------------------------------------------

    println!("\n{}", "=".repeat(60));

    println!("\nResearcher output:");
    let snippet = &research_result.output[..research_result.output.len().min(400)];
    println!("{}", snippet);

    println!("\nAnalyst briefing:");
    println!("{}", "-".repeat(60));
    println!("{}", analyst_result.output);
    println!("{}", "-".repeat(60));

    let total_input =
        research_result.token_usage.input_tokens + analyst_result.token_usage.input_tokens;
    let total_output =
        research_result.token_usage.output_tokens + analyst_result.token_usage.output_tokens;
    println!("\nTotal tokens -- input: {}, output: {}", total_input, total_output);
}
