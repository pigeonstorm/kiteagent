# kite-gear

Small Rust library compiled to **WebAssembly** so the subscription dashboard can suggest **kite area (m²)** and **wing area (m²)** from **true wind (knots)**, **rider weight (kg)**, and whether the rider is on a **foil** (kites only).

## Intent

- **Single source of truth** for gear sizing rules in the browser: same thresholds and weight scaling as the rest of the product expects, without reimplementing the math in JavaScript.
- **Reference rider 80 kg**: heavier riders need more wind to move into a smaller-kite band (or get a larger kite at the same wind); lighter riders the opposite. Foil mode reduces effective weight by 20 kg (floored at 30 kg) so recommendations skew smaller, since foil lift lets people ride less kite.
- **Not medical or safety advice**: discrete steps (5, 7, 9, 12, 14 m² for kites; 3.2–7 m² for wings) are a simple guide; real conditions, skill, kite model, and local rules matter.

## API (Wasm)

`wasm-bindgen` exports:

- `kite_size(wind_kn, rider_kg, foil) -> f64` — returns `0.0` when wind is below the useful range.
- `wing_size(wind_kn, rider_kg) -> f64` — same idea for wings.

The server serves the bundle as `/kite-gear.js` and `/kite-gear.wasm` (see `server/src/routes.rs`); the static UI loads them from there.

## Build

From the repo root (requires [wasm-pack](https://rustwasm.github.io/wasm-pack/installer/) and `rustup target add wasm32-unknown-unknown`):

```bash
rake kite-gear
```

or:

```bash
wasm-pack build --target web kite-gear
```

Artifacts land in **`kite-gear/pkg/`** (`kite_gear.js`, `kite_gear_bg.wasm`). The server resolves those paths relative to its working directory at deploy time.

## Tests

Native Rust tests (no browser) exercise the lookup tables and weight/foil behavior:

```bash
cargo test -p kite-gear
```
