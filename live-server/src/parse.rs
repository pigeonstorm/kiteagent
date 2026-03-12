/// Scrapes the ARL:UT Lake Travis weewx/NeoWX page.
///
/// Page structure: each metric lives in a `.crd` div whose title is an `<h4>`
/// (or `<h5>` for the wind-chill/heat-index pair).  The current value is in a
/// `<span class="weatherdata">` and detail rows are a `<table class="meta">`.
/// Wind direction degrees are embedded in icon class names like
/// `wi-wind from-158-deg`.
use anyhow::{Context, Result};
use chrono::Utc;
use regex::Regex;
use scraper::{Html, Selector};
use std::collections::HashMap;

use crate::WeatherReading;

const MPH_TO_KN: f64 = 0.868976;

// ── card data collected from the DOM ─────────────────────────────────────────

struct Card {
    /// Text content of the `<span class="weatherdata">` (whitespace-normalised).
    weatherdata: String,
    /// Text content of every `<td>` in the card's meta table, in order.
    tds: Vec<String>,
    /// Degree values extracted from `wi-wind from-NNN-deg` icon classes, in DOM
    /// order: [current, hi, vector-avg] for the Wind card.
    wind_deg_icons: Vec<i32>,
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn first_f64(s: &str) -> Option<f64> {
    Regex::new(r"-?\d+(?:\.\d+)?")
        .ok()?
        .find(s)?
        .as_str()
        .parse()
        .ok()
}

/// Scan consecutive td pairs `[label, value, …]` for a matching label prefix.
fn td_value_after<'a>(tds: &'a [String], label_prefix: &str) -> Option<&'a str> {
    tds.windows(2)
        .find(|w| w[0].starts_with(label_prefix))
        .map(|w| w[1].as_str())
}

// ── DOM traversal ─────────────────────────────────────────────────────────────

fn extract_cards(doc: &Html) -> HashMap<String, Card> {
    let crd_sel = Selector::parse(".crd").unwrap();
    let h4_sel = Selector::parse("h4").unwrap();
    let h5_sel = Selector::parse("h5").unwrap();
    let wd_sel = Selector::parse(".weatherdata").unwrap();
    let td_sel = Selector::parse("td").unwrap();
    let icon_sel = Selector::parse("i[class]").unwrap();
    let deg_re = Regex::new(r"from-(\d+)-deg").unwrap();

    let mut cards = HashMap::new();

    for card in doc.select(&crd_sel) {
        // Title: prefer h4, fall back to h5 (wind-chill card uses h5)
        let title = card
            .select(&h4_sel)
            .next()
            .or_else(|| card.select(&h5_sel).next())
            .map(|el| {
                el.text()
                    .collect::<String>()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();

        if title.is_empty() {
            continue;
        }

        let weatherdata = card
            .select(&wd_sel)
            .next()
            .map(|el| {
                el.text()
                    .collect::<String>()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();

        // Skip graph cards (e.g. "Wind" graph, "Barometer" graph) — they have no .weatherdata
        if weatherdata.is_empty() {
            continue;
        }

        let tds: Vec<String> = card
            .select(&td_sel)
            .map(|el| {
                el.text()
                    .collect::<String>()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();

        let wind_deg_icons: Vec<i32> = card
            .select(&icon_sel)
            .filter_map(|el| {
                let class = el.value().attr("class")?;
                let caps = deg_re.captures(class)?;
                caps[1].parse().ok()
            })
            .collect();

        cards.insert(title, Card { weatherdata, tds, wind_deg_icons });
    }

    cards
}

// ── public entry point ────────────────────────────────────────────────────────

pub fn scrape_html(html: &str) -> Result<WeatherReading> {
    let doc = Html::parse_document(html);

    // Station local time is the second <h2> on the page
    let h2_sel = Selector::parse("h2").unwrap();
    let station_time = doc
        .select(&h2_sel)
        .nth(1)
        .map(|el| el.text().collect::<String>().trim().to_string())
        .unwrap_or_default();

    let cards = extract_cards(&doc);

    // ── Outside Temperature ────────────────────────────────────────────────
    let temp_card = cards
        .iter()
        .find(|(k, _)| {
            let kl = k.to_lowercase();
            kl.contains("temperature") && !kl.contains("lake")
        })
        .map(|(_, v)| v)
        .context("missing Temperature card")?;
    let temperature_f = first_f64(&temp_card.weatherdata).unwrap_or(0.0);

    // ── Humidity ───────────────────────────────────────────────────────────
    let hum_card = cards
        .iter()
        .find(|(k, _)| k.to_lowercase().contains("humidity"))
        .map(|(_, v)| v)
        .context("missing Humidity card")?;
    let humidity_pct = first_f64(&hum_card.weatherdata).unwrap_or(0.0);

    // ── Barometer ──────────────────────────────────────────────────────────
    let baro_card = cards
        .iter()
        .find(|(k, _)| k.to_lowercase().contains("barometer"))
        .map(|(_, v)| v)
        .context("missing Barometer card")?;
    let barometer_inhg = first_f64(&baro_card.weatherdata).unwrap_or(0.0);
    let barometer_trend = td_value_after(&baro_card.tds, "Trend")
        .and_then(first_f64);

    // ── Rain ───────────────────────────────────────────────────────────────
    let rain_card = cards
        .iter()
        .find(|(k, _)| {
            let kl = k.to_lowercase();
            kl.contains("rain") && !kl.contains("rate")
        })
        .map(|(_, v)| v)
        .context("missing Rain card")?;
    let rain_in = first_f64(&rain_card.weatherdata).unwrap_or(0.0);
    let rain_rate_in_hr = td_value_after(&rain_card.tds, "Rain Rate")
        .and_then(first_f64)
        .unwrap_or(0.0);

    // ── Wind Chill / Heat Index ────────────────────────────────────────────
    let wchi_card = cards
        .iter()
        .find(|(k, _)| {
            let kl = k.to_lowercase();
            kl.contains("chill") || kl.contains("heat index")
        })
        .map(|(_, v)| v)
        .context("missing Wind Chill card")?;
    // weatherdata is "75.1°F | 74.9°F"
    let wchi_parts: Vec<f64> = wchi_card
        .weatherdata
        .split('|')
        .filter_map(|s| first_f64(s))
        .collect();
    let wind_chill_f = wchi_parts.first().copied().unwrap_or(0.0);
    let heat_index_f = wchi_parts.get(1).copied().unwrap_or(0.0);

    // ── Dewpoint ───────────────────────────────────────────────────────────
    let dew_card = cards
        .iter()
        .find(|(k, _)| k.to_lowercase().contains("dewpoint"))
        .map(|(_, v)| v)
        .context("missing Dewpoint card")?;
    let dewpoint_f = first_f64(&dew_card.weatherdata).unwrap_or(0.0);

    // ── Wind ───────────────────────────────────────────────────────────────
    // Title is "Wind" (not "Wind Chill", not "Heat Index")
    let wind_card = cards
        .iter()
        .find(|(k, _)| {
            let kl = k.to_lowercase();
            kl.contains("wind") && !kl.contains("chill") && !kl.contains("heat")
        })
        .map(|(_, v)| v)
        .context("missing Wind card")?;

    // weatherdata: "13 mph SSE" — after whitespace normalisation (ARL page uses mph)
    let wd_text = &wind_card.weatherdata;
    let wind_speed_kn = first_f64(wd_text).unwrap_or(0.0) * MPH_TO_KN;
    let wind_direction = Regex::new(r"\b([NSEW]{1,3})\b")
        .ok()
        .and_then(|re| re.find(wd_text))
        .map(|m| m.as_str().to_string())
        .unwrap_or_default();
    // Degrees come from "wi-wind from-NNN-deg" icons: [current, hi, vector_avg]
    let wind_direction_deg = wind_card.wind_deg_icons.first().copied().unwrap_or(0);

    let wind_avg_kn = td_value_after(&wind_card.tds, "Avg")
        .and_then(first_f64)
        .unwrap_or(0.0) * MPH_TO_KN;
    let wind_hi_kn = td_value_after(&wind_card.tds, "Hi")
        .and_then(first_f64)
        .unwrap_or(0.0) * MPH_TO_KN;
    let wind_hi_dir_deg = wind_card.wind_deg_icons.get(1).copied().unwrap_or(0);
    let wind_rms_kn = td_value_after(&wind_card.tds, "RMS")
        .and_then(first_f64)
        .unwrap_or(0.0) * MPH_TO_KN;
    let wind_vector_avg_kn = td_value_after(&wind_card.tds, "Vector Avg")
        .and_then(first_f64)
        .unwrap_or(0.0) * MPH_TO_KN;
    let wind_vector_dir_deg = wind_card.wind_deg_icons.get(2).copied().unwrap_or(0);

    Ok(WeatherReading {
        id: None,
        scraped_at: Utc::now(),
        station_time,
        wind_speed_kn,
        wind_direction,
        wind_direction_deg,
        wind_avg_kn,
        wind_hi_kn,
        wind_hi_dir_deg,
        wind_rms_kn,
        wind_vector_avg_kn,
        wind_vector_dir_deg,
        temperature_f,
        humidity_pct,
        barometer_inhg,
        barometer_trend,
        rain_in,
        rain_rate_in_hr,
        wind_chill_f,
        heat_index_f,
        dewpoint_f,
    })
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_first_f64() {
        assert_eq!(first_f64("13 mph SSE"), Some(13.0));
        assert_eq!(first_f64("29.957 inHg"), Some(29.957));
        assert_eq!(first_f64("0.00 in"), Some(0.0));
        assert_eq!(first_f64("no numbers"), None);
    }

    /// Wind data card appears before graph cards; we must not overwrite it with
    /// the graph card (which has no .weatherdata).
    #[test]
    fn wind_card_not_overwritten_by_graph() {
        let html = r#"
        <html><body>
        <h2>Station</h2><h2>03/02/2026 11:20 PM</h2>
        <div class="crd">
            <h4><i class="wi wi-strong-wind"></i> Wind</h4>
            <div class="crd-content">
                <span class="weatherdata">10 mph S <br><i class="wi wi-wind from-180-deg"></i></span>
                <table class="meta">
                    <tr><td>Avg:</td><td>11 mph</td><td></td></tr>
                    <tr><td>Hi:</td><td>35 mph<br> 160° <i class="wi wi-wind from-160-deg"></i></td><td>(21:07)</td></tr>
                    <tr><td>RMS:</td><td>11 mph</td><td></td></tr>
                    <tr><td>Vector Avg:</td><td>10 mph<br> 170° <i class="wi wi-wind from-170-deg"></i></td><td></td></tr>
                </table>
            </div>
        </div>
        <div class="crd graph-crd">
            <h4>Wind</h4>
            <a href="daywind.png"><img src="daywind.png" alt="Wind"></a>
        </div>
        <div class="crd"><h4>Outside Temperature</h4><div class="crd-content"><span class="weatherdata">68.7°F</span></div></div>
        <div class="crd"><h4>Outside Humidity</h4><div class="crd-content"><span class="weatherdata">83%</span></div></div>
        <div class="crd"><h4>Barometer</h4><div class="crd-content"><span class="weatherdata">29.995 inHg</span><table class="meta"><tr><td>Trend (3.0 hours):</td><td>0.023 inHg</td></tr></table></div></div>
        <div class="crd"><h4>Rain</h4><div class="crd-content"><span class="weatherdata">0.00 in</span><table class="meta"><tr><td>Rain Rate:</td><td>0.00 in/hr</td></tr></table></div></div>
        <div class="crd"><h5>Wind Chill | Heat Index</h5><div class="crd-content"><span class="weatherdata">68.7°F | 69.2°F</span></div></div>
        <div class="crd"><h4>Dewpoint</h4><div class="crd-content"><span class="weatherdata">63.3°F</span></div></div>
        </body></html>
        "#;
        let r = scrape_html(html).unwrap();
        assert!((r.wind_speed_kn - 8.69).abs() < 0.1); // 10 mph ≈ 8.69 kts
        assert_eq!(r.wind_direction, "S");
        assert_eq!(r.wind_direction_deg, 180);
        assert!((r.wind_avg_kn - 9.56).abs() < 0.1);  // 11 mph ≈ 9.56 kts
        assert!((r.wind_hi_kn - 30.41).abs() < 0.1); // 35 mph ≈ 30.41 kts
        assert_eq!(r.wind_hi_dir_deg, 160);
        assert!((r.wind_rms_kn - 9.56).abs() < 0.1);
        assert!((r.wind_vector_avg_kn - 8.69).abs() < 0.1);
        assert_eq!(r.wind_vector_dir_deg, 170);
    }
}
