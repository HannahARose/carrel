# plant-monitor

A soil moisture monitor for houseplants: an ESP32 reads a capacitive sensor every 10 minutes and posts the reading to a small web page.

## Building
- `cargo build --release` for the firmware
- Wire the sensor to GPIO 34 and 3.3 V

## To do
- Battery power and deep sleep
