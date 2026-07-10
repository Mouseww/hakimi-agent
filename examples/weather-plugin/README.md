# Weather Plugin

Query weather information for cities worldwide.

## Overview

This plugin demonstrates:
- Structured data handling with serde
- Mock API responses
- Formatted output generation

## Build

```bash
cargo build --target wasm32-wasip1 --release
```

## Usage

Load and execute the plugin:

```bash
hakimi plugin install target/wasm32-wasip1/release/weather_plugin.wasm
hakimi plugin execute weather-plugin
```

## Example Output

```
🌤️ Weather Report

📍 City: Beijing
🌡️ Temperature: 25.3°C
☁️ Conditions: Clear sky
💧 Humidity: 45%
💨 Wind Speed: 3.2 m/s

Data provided by Weather Plugin v0.1.0
```

## Future Enhancements

- Real API integration (OpenWeatherMap, etc.)
- Multi-city queries
- Historical weather data
- Weather alerts and forecasts
