# ec-su_abx35-win

A Windows control & monitoring solution for the onboard Embedded Controller (EC) on Sixunited's SU_AXB35 boards. A distant relative of [ec-su_axb35-linux](https://github.com/cmetz/ec-su_axb35-linux), consists of a server and a GUI client (not yet available).

The whole thing would've been impossible without the work done in [NoteBook FanControl](https://github.com/hirschmann/nbfc) and [WinRing0 library](https://github.com/GermanAizek/WinRing0).


## Features

- **Full EC functionality**: Fan control (auto/fixed/curve) and monitoring, power control (3 standard presets), APU temperature reading
- **HTTP REST API**: All requests and responses use JSON format
- **Runs as a system service**: Very low memory footprint, doesn't do anything unless asked
- **Client is optional**: Running a GUI monitoring/configuration tool is not required


## Requirements

- EC Firmware 1.04 or higher (get it [here](https://strixhalo-homelab.d7.wtf/Hardware/Boards/Sixunited-AXB35/Firmware))
- Windows 11 running on SU_AXB35 board (could work on W10 too, untested)
- Administrator privileges for installation
- Disabled Secure Boot for WinRing0 driver loading


## Configuration

The server loads configuration from `%SYSTEMDRIVE%\ProgramData\ec-su_axb35-win\config.json`. If the file doesn't exist, it will be created with default values:

```json
{
  "host": "127.0.0.1",
  "port": 8395,
  "log_path": "C:\\ProgramData\\ec-su_axb35-win\\server.log",
  "driver_path": "C:\\ProgramData\\ec-su_axb35-win\\winring0"
}
```


## Safety & Implementation Notes

- All EC operations are performed synchronously one by one
- The server will exit if it cannot access the EC or load the required driver
- All states are being kept on the server side in its config and re-applied on start
- HTTP REST API does not have any authorization implemented, unless you're sure that this is what you want, never set the server to listen on public interfaces


## API Endpoints

#### General
- **GET** `/status` - Get EC firmware version and status
- **GET** `/metrics` - Get combined monitoring data (power mode, temperature, all fan data)

#### APU Power Mode
- **GET/POST** `/apu/power_mode` - Get or set current power mode (balanced/performance/quiet)
- **GET** `/apu/temp` - Get APU temperature

#### Fan Control (X = 1, 2, or 3)
- **GET** `/fanX/rpm` - Get fan RPM
- **GET/POST** `/fanX/mode` - Get or set fan mode (auto/fixed/curve)
- **GET/POST** `/fanX/level` - Get or set fan level (0-5) for `fixed` mode
- **GET/POST** `/fanX/rampup_curve` - Get or set fan rampup curve (5 temperature thresholds) for `curve` mode
- **GET/POST** `/fanX/rampdown_curve` - Get or set fan rampdown curve (5 temperature thresholds) for `curve` mode

#### OpenAPI Specs

There are [OpenAPI specifications available in the repo](https://raw.githubusercontent.com/deseven/ec-su_axb35-win/refs/heads/main/server/openapi.yaml) with full route descriptions and request/response examples. You can simply copy the URL and import it in [the Swagger Editor](https://editor.swagger.io/) or any other OpenAPI-compatible editor/viewer.


## Curve Fan Mode

The curve fan mode provides automatic fan speed control based on APU temperature using customizable temperature thresholds:

- **Rampup curve**: 5 temperature thresholds (°C) that trigger fan level increases (levels 1-5)
- **Rampdown curve**: 5 temperature thresholds (°C) that trigger fan level decreases (levels 1-5)
- **Real-time monitoring**: Server continuously monitors APU temperature and adjusts fan speeds accordingly
- **Hysteresis**: Separate rampup/rampdown curves prevent rapid fan speed oscillation
- **Per-fan configuration**: Each fan (1, 2, 3) can have independent curve settings

**Default Curve Values for Fan 1 & 2:**
- Rampup: [60, 70, 83, 95, 97]°C
- Rampdown: [40, 50, 80, 94, 96]°C

**Default Curve Values for Fan 3:**
- Rampup: [20, 60, 83, 95, 97]°C
- Rampdown: [0, 50, 80, 94, 96]°C

#### Curve Mode Operation

1. Set fan mode to "curve" using the `/fanX/mode` endpoint
2. Optionally customize rampup/rampdown curves using the curve endpoints
3. Server automatically monitors APU temperature every second
4. Fan levels adjust based on temperature crossing the configured thresholds
5. All curve settings are saved to config and restored on server restart


## Error Handling

The server returns appropriate HTTP status codes:
- `200 OK` - Successful operation
- `400 Bad Request` - Invalid request data
- `500 Internal Server Error` - EC communication or server error

All error responses include a JSON object with an `error` field describing the issue.


## Logging

The server logs all operations with timestamps to:
- Standard output (if run in a console)
- Log file at `%SYSTEMDRIVE%\ProgramData\ec-su_axb35-win\server.log`

The log file is overwritten on each server restart.


## Testing

There are a couple of PoSh scripts available in `%SYSTEMDRIVE%\ProgramData\ec-su_axb35-win\scripts`. Use them to quickly test metrics output and fan levels.


## Building and Running (server)

1. Ensure you have Rust installed and you can build a simple "hello world" app.
2. Build the project:
   ```bash
   cargo build --release
   ```
3. Run as Administrator:
   ```bash
   cargo run --release
   ```


## Help, support and contributions
If you found a bug, have a suggestion or some question, feel free to [create an issue](https://github.com/deseven/ec-su_axb35-win/issues/new) in this repo.

There is also a Strix Halo HomeLab Discord server you can join - https://discord.gg/pnPRyucNrG