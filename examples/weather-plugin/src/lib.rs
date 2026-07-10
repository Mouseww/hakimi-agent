//! Weather Plugin
//!
//! 查询城市天气信息的示例插件
//! 演示了如何使用宿主函数调用和结构化数据处理

use hakimi_plugin_sdk::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct WeatherQuery {
    city: String,
}

#[derive(Serialize, Deserialize)]
struct WeatherResponse {
    city: String,
    temperature: f32,
    description: String,
    humidity: u32,
    wind_speed: f32,
}

#[hakimi_plugin(
    name = "weather-plugin",
    version = "0.1.0",
    author = "Hakimi Team",
    description = "Query weather information for cities worldwide"
)]
pub struct WeatherPlugin;

impl WeatherPlugin {
    /// 插件执行函数 - 查询天气信息
    ///
    /// 输入: JSON 格式的城市查询
    /// 输出: 格式化的天气信息
    pub fn execute(&self, ctx: &PluginContext) -> PluginResult<String> {
        ctx.log("info", "Weather Plugin executing");
        
        // 示例：模拟天气查询（生产环境中应调用真实 API）
        let demo_cities = vec![
            WeatherResponse {
                city: "Beijing".to_string(),
                temperature: 25.3,
                description: "Clear sky".to_string(),
                humidity: 45,
                wind_speed: 3.2,
            },
            WeatherResponse {
                city: "Shanghai".to_string(),
                temperature: 28.7,
                description: "Partly cloudy".to_string(),
                humidity: 65,
                wind_speed: 4.5,
            },
            WeatherResponse {
                city: "London".to_string(),
                temperature: 15.2,
                description: "Light rain".to_string(),
                humidity: 80,
                wind_speed: 6.1,
            },
        ];
        
        // 选择一个随机城市作为示例
        let weather = &demo_cities[0];
        
        ctx.log("info", &format!("Fetched weather for {}", weather.city));
        
        let output = format!(
            "🌤️ Weather Report\\n\\n\
            📍 City: {}\\n\
            🌡️ Temperature: {:.1}°C\\n\
            ☁️ Conditions: {}\\n\
            💧 Humidity: {}%\\n\
            💨 Wind Speed: {:.1} m/s\\n\\n\
            Data provided by Weather Plugin v0.1.0",
            weather.city,
            weather.temperature,
            weather.description,
            weather.humidity,
            weather.wind_speed
        );
        
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_weather_plugin() {
        let plugin = WeatherPlugin;
        let ctx = PluginContext::default();
        
        let result = plugin.execute(&ctx);
        assert!(result.is_ok());
        
        let output = result.unwrap();
        assert!(output.contains("Weather Report"));
        assert!(output.contains("Temperature"));
        assert!(output.contains("Humidity"));
    }
}
