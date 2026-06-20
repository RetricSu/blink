# Blink - ESP32-C3 Smart Display Gadget

A simple hardware gadget built with ESP32-C3 that displays time, quotes, and features a 25-minute countdown timer with button interaction.

## Features

- **Time Display**: Shows current time on the OLED display
- **Quote Display**: Displays inspirational quotes
- **Pomodoro Timer**: 25-minute countdown session timer
- **Button Interaction**: Single button to cycle through different modes
- **OLED Display**: 128x64 pixel SSD1306 display for clear text output

## Hardware Requirements

- **ESP32-C3** development board
- **SSD1306 OLED Display** (128x64 pixels, I2C interface)
- **Push Button** for mode switching
- **Breadboard and Jumper Wires** for connections
- **USB-C Cable** for programming and power

## Pin Connections

| ESP32-C3 Pin | Component | Description |
|--------------|-----------|-------------|
| GPIO8        | SSD1306 SDA | I2C Data Line |
| GPIO9        | SSD1306 SCL | I2C Clock Line |
| GPIO10       | Button     | Mode Switch Button |
| 3.3V         | SSD1306 VCC | Power Supply |
| GND          | SSD1306 GND | Ground |

## Software Requirements

- **Rust** (stable channel)
- **espflash** - ESP32 flashing tool
- **cargo-espflash** - Cargo integration for espflash

### Installation

```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install espflash
cargo install espflash

# Add ESP32 target
rustup target add riscv32imc-unknown-none-elf
```

## Building and Flashing

### Quick Start
```bash
# Clone the repository
git clone <your-repo-url>
cd blink

# Build and flash to ESP32-C3
cargo run
```

### Manual Build
```bash
# Build the project
cargo build --release

# Flash manually
espflash flash --monitor target/riscv32imc-unknown-none-elf/release/blink
```

## Usage

### Button Functions
- **Single Click**: Cycle through display modes
  - Time display
  - Quote display  
  - Timer mode
- **Long Press**: Start/stop 25-minute countdown timer
- **Double Click**: Reset timer

### Display Modes

1. **Time Mode**: Shows current time in HH:MM format
2. **Quote Mode**: Displays inspirational quotes with scrolling text
3. **Timer Mode**: Shows countdown timer with progress bar
4. **Price Mode**: Cycles through BTC, CKB, and Gold prices (basic version uses simulated prices)

## Project Structure

```
blink/
├── src/
│   ├── bin/
│   │   └── main.rs          # Main application code
│   └── lib.rs               # Library code (if any)
├── .cargo/
│   └── config.toml          # Cargo configuration for ESP32
├── Cargo.toml               # Project dependencies
├── build.rs                 # Build script
└── README.md               # This file
```

## Dependencies

- **esp-hal**: ESP32 hardware abstraction layer
- **embedded-graphics**: Graphics library for embedded displays
- **ssd1306**: SSD1306 OLED display driver
- **esp-backtrace**: Error handling and backtraces
- **log**: Logging framework

## Development

### Adding New Features

1. **New Display Mode**: Add a new mode to the display cycle
2. **Button Actions**: Implement additional button interactions
3. **Quotes**: Add more quotes to the quote database
4. **Timer Customization**: Modify timer duration or add multiple timers

### Debugging

The project includes serial output for debugging:
```bash
# Monitor serial output
cargo run -- --monitor
```

Log levels can be controlled via the `ESP_LOG` environment variable.

## Troubleshooting

### Common Issues

1. **Display Not Working**
   - Check I2C connections (SDA/SCL)
   - Verify power supply (3.3V)
   - Ensure correct I2C address

2. **Button Not Responding**
   - Check button wiring and pull-up resistors
   - Verify GPIO pin configuration

3. **Flashing Issues**
   - Ensure ESP32-C3 is in download mode
   - Check USB connection and drivers
   - Verify espflash installation

### Serial Monitor Output

The device outputs debug information via serial:
```
INFO - Hello world!
INFO - Button pressed
INFO - Mode changed to: Timer
```

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Test thoroughly on hardware
5. Submit a pull request

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Acknowledgments

- ESP-RS team for the excellent esp-hal framework
- Embedded Graphics community for the display library
- Rust embedded community for tools and examples

## Support

For issues and questions:
- Check the troubleshooting section
- Review ESP32-C3 documentation
- Open an issue on GitHub

---

**Note**: This is a work in progress. The current code shows "Hello World!" on the display. The full functionality (time, quotes, timer) needs to be implemented based on the requirements described in this README.
