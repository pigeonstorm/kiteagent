use wasm_bindgen::prelude::*;

const REF_WEIGHT_KG: f64 = 80.0;

fn weight_factor(rider_kg: f64) -> f64 {
    REF_WEIGHT_KG / rider_kg
}

/// Kite size (m^2) for the given wind and rider weight.
/// Returns 0.0 when wind is below the minimum useful range.
#[wasm_bindgen]
pub fn kite_size(wind_kn: f64, rider_kg: f64) -> f64 {
    let f = weight_factor(rider_kg);
    if wind_kn >= 28.0 * f {
        5.0
    } else if wind_kn >= 19.0 * f {
        7.0
    } else if wind_kn >= 15.0 * f {
        9.0
    } else if wind_kn >= 12.0 * f {
        12.0
    } else if wind_kn >= 8.0 * f {
        14.0
    } else {
        0.0
    }
}

/// Wing size (m^2) for the given wind and rider weight.
/// Returns 0.0 when wind is below the minimum useful range.
#[wasm_bindgen]
pub fn wing_size(wind_kn: f64, rider_kg: f64) -> f64 {
    let f = weight_factor(rider_kg);
    if wind_kn >= 28.0 * f {
        3.2
    } else if wind_kn >= 22.0 * f {
        4.2
    } else if wind_kn >= 16.0 * f {
        5.0
    } else if wind_kn >= 10.0 * f {
        7.0
    } else {
        0.0
    }
}
