/// WMO weather interpretation code → human-readable description.
pub fn weather_code_desc(code: i64) -> &'static str {
    match code {
        0 => "Clear sky",
        1 => "Mainly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 | 48 => "Foggy",
        51 | 53 | 55 => "Drizzle",
        61 | 63 | 65 => "Rain",
        71 | 73 | 75 => "Snowfall",
        77 => "Snow grains",
        80 | 81 | 82 => "Rain showers",
        85 | 86 => "Snow showers",
        95 => "Thunderstorm",
        96 | 99 => "Thunderstorm with hail",
        _ => "Unknown conditions",
    }
}

pub async fn call(args: &serde_json::Value, client: &reqwest::Client) -> super::McpCallResponse {
    let unit = args.get("unit").and_then(|v| v.as_str()).unwrap_or("fahrenheit");
    let temp_unit = if unit == "celsius" { "celsius" } else { "fahrenheit" };
    let symbol = if unit == "celsius" { "°C" } else { "°F" };

    let url = format!(
        "https://api.open-meteo.com/v1/forecast\
         ?latitude=43.1935&longitude=-112.3490\
         &current=temperature_2m,weather_code,relative_humidity_2m,wind_speed_10m\
         &temperature_unit={temp_unit}&wind_speed_unit=mph&timezone=America%2FDenver"
    );

    match client.get(&url).send().await {
        Err(e) => super::McpCallResponse {
            content: format!("Failed to reach Open-Meteo API: {e}"),
            is_error: true,
        },
        Ok(resp) => {
            if !resp.status().is_success() {
                return super::McpCallResponse {
                    content: format!("Open-Meteo returned HTTP {}", resp.status()),
                    is_error: true,
                };
            }
            match resp.json::<serde_json::Value>().await {
                Err(e) => super::McpCallResponse {
                    content: format!("Failed to parse weather response: {e}"),
                    is_error: true,
                },
                Ok(json) => {
                    let cur = &json["current"];
                    let temp = cur["temperature_2m"].as_f64().unwrap_or(0.0);
                    let humid = cur["relative_humidity_2m"].as_f64().unwrap_or(0.0);
                    let wind = cur["wind_speed_10m"].as_f64().unwrap_or(0.0);
                    let code = cur["weather_code"].as_i64().unwrap_or(0);
                    let unit_label = if unit == "celsius" { "Celsius" } else { "Fahrenheit" };
                    super::McpCallResponse {
                        content: format!(
                            "Current weather in Blackfoot, Idaho (temperatures in {unit_label}):\n\
                             Temperature : {temp:.1} {symbol}\n\
                             Conditions  : {}\n\
                             Humidity    : {humid:.0}%\n\
                             Wind Speed  : {wind:.1} mph",
                            weather_code_desc(code),
                        ),
                        is_error: false,
                    }
                }
            }
        }
    }
}

pub const README: &str = r#"
# Current Temperature — Blackfoot, Idaho

Fetches live weather conditions in **Blackfoot, Idaho** using the
[Open-Meteo](https://open-meteo.com/) free weather API. No API key is required.

## What it returns

| Field | Description |
|---|---|
| Temperature | Current air temperature at 2 m above ground |
| Conditions | Human-readable sky/weather description |
| Humidity | Relative humidity (%) |
| Wind Speed | 10-metre wind speed in mph |

## Parameters

| Name | Type | Default | Description |
|---|---|---|---|
| `unit` | `"fahrenheit"` \| `"celsius"` | `"fahrenheit"` | Temperature unit |

## Example usage

Ask the AI:
> *"What's the weather like in Blackfoot right now?"*

The AI will call this tool and include the result in its reply.

## Data source

Weather data is provided by [Open-Meteo](https://open-meteo.com/) — a free,
open-source weather API with no usage limits for non-commercial use.

Coordinates used: **43.1935°N, 112.3490°W** (Blackfoot, ID, United States)
"#;
