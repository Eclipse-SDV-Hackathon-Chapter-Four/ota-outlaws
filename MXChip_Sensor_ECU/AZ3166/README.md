<!--
  Copyright (c) 2024 Eclipse Foundation
 
  This program and the accompanying materials are made available 
  under the terms of the MIT license which is available at
  https://opensource.org/license/mit.
 
  SPDX-License-Identifier: MIT
 
  Contributors: 
      Frédéric Desbiens - Initial version.

      Andy Riexinger - Documentation for Mac M1.
      Uttarkar Sopan - Feature enhancement for injecting fault

-->

# Eclipse ThreadX IoT DevKit Starter Application

This starter application is an adaptation of a sample developed originally by Microsoft. The original code can be found here:

[https://github.com/eclipse-threadx/getting-started](https://github.com/eclipse-threadx/getting-started)

We removed code providing support for Azure IoT Cloud. The only board supported is the MXChip AZ3166 for the time being.

## Cloning this repository
Eclipse ThreadX and Eclipse ThreadX NetX Duo are included as submodules.

When cloning, you must specify the `--recurse-submodules` option to get the code for the submodules. If you forget this option, just run the following commands in the root folder of your clone. 

```
git submodule init
git submodule update --recursive
```

## Prerequisites

### Computer
Theoretically, any recent laptop running Windows 11, Linux, or MacOS should do.

We tested on the following environments:
- Windows 11 24H2 (Version 10.0.26100.2161)
- Ubuntu 22.04.5 LTS (Windows Subsystem for Linux version 2.3.24.0)
- Mac M1, macOS Sonoma 14.6.1 (23G93), native
- Mac M1, macOS Sonoma 14.6.1 (23G93), Parallels Desktop for Mac Version 17.1.7 (51588), Ubuntu 22.04.5 LTS


### IoT DevKit
This sample is preconfigured to work with the [MXChip AZ3166 board](https://docs.mxchip.com/en/nr6ggk/blyezpv6gkqicywi.html).

This board can run Eclipse ThreadX but is also Arduino compatible.

The board features a [STM32F412RG MCU](https://www.st.com/en/microcontrollers-microprocessors/stm32f412rg.html) from STMicroelectronics. The MCU is clocked at 100Mhz and comes with 1 Mbyte of flash memory and 256Kbytes of SRAM.

The AZ3166 has the following hardware onboard:
- 128*64 dot matrix OLED display: VGM128064
- RGB LED lights controlled by P9813:
- Temperature and Humidity Sensor: HTS221
- Atmospheric pressure sensor: LPS22HB
- Bidirectional mono audio ADC/DAC: NAU88C10, microphone and 3.5mm headphone jack
- Six-axis accelerometer: LSM6DSL
- Geomagnetic sensor: LIS2MDL
- DC motor
- Two buttons
- three LED indicators

The board also features WiFi connectivity. However, only 2.4Ghz networks are supported.

This starter application is preconfigured to provide access to some, but not all, of the peripherals above.

### Developer tools
In terms of tooling, all you need to work on the challenge is CMake, Ninja, and a suitable C compiler. Naturally, having Git installed could help as well. ;-)

The source code for ThreadX and related modules is very portable and compliant with all "required" and "mandatory" rules of MISRA-C:2004 and MISRA C:2012. Most modern C compilers should be able to compile it. The official build pipelines rely on Arm's embedded GNU toolchain.

Below are instructions to install the tools.

**Ubuntu**
```
apt install ninja-build cmake 
```

Then, download and install Arm's embedded GNU toolchain, available at [https://developer.arm.com/downloads/-/arm-gnu-toolchain-downloads](https://developer.arm.com/downloads/-/arm-gnu-toolchain-downloads)

The following will download and unpack version 13.3.rel1 of the software to `/opt`.
``` 
wget https://developer.arm.com/-/media/Files/downloads/gnu/13.3.rel1/binrel/arm-gnu-toolchain-13.3.rel1-x86_64-arm-none-eabi.tar.xz
sudo tar xJf arm-gnu-toolchain-13.3.rel1-x86_64-arm-none-eabi.tar.xz -C /opt
```

To test, you can run the following commands:
```
export PATH=$PATH:/opt/arm-gnu-toolchain-13.3.rel1-x86_64-arm-none-eabi/bin
arm-none-eabi-gcc --version
```

**MacOS**

For Mac M1, we tested using version 14.2.rel1. Below are links to download. The install is similar to Linux. 

- Mac M1 native: [arm-gnu-toolchain-14.2.rel1-darwin-arm64-arm-none-eabi.pkg](https://developer.arm.com/-/media/Files/downloads/gnu/14.2.rel1/binrel/arm-gnu-toolchain-14.2.rel1-darwin-arm64-arm-none-eabi.pkg)
- Ubuntu 22.04.5 on Mac M1 via Parallels Desktop: [arm-gnu-toolchain-14.2.rel1-aarch64-arm-none-eabi.tar.xz](https://developer.arm.com/-/media/Files/downloads/gnu/14.2.rel1/binrel/arm-gnu-toolchain-14.2.rel1-aarch64-arm-none-eabi.tar.xz)


**Windows**
```
winget install --id=Arm.GnuArmEmbeddedToolchain  -e
winget install --id=Ninja-build.Ninja  -e
winget install --id=Kitware.CMake  -e
```

## Compiling and running the application
To compile the application, use the provided scripts in the `MXChip/AZ3166/scripts` folder.

### Windows (PowerShell)
You can build the application using `build.ps1`. It accepts a `-Config` parameter to select the application version (`starter`, `arcade`, `telemetry`, `mqtt`, or `guardian`). Each configuration has its own build directory under `build`, so cached outputs from another configuration or worktree are not reused.
```powershell
.\scripts\build.ps1 -Config starter
```

For the MQTT configuration, place Wi-Fi settings in an untracked
`app/mqtt/cloud_config.local.h` file. Define `HOSTNAME`, `WIFI_SSID`, and
`WIFI_PASSWORD` there; this file is ignored by Git so credentials are not
committed. The AZ3166 OLED cycles through a welcome page and the onboard sensor
readings, while sensor and networking logs are written to the UART console.

For the `guardian` configuration, provide Wi-Fi settings and the IPv4 address
of the host running the OTA Outlaws campaign bridge in an untracked
`app/guardian/cloud_config.local.h` file:

```c
#define WIFI_SSID "your-2.4GHz-network"
#define WIFI_PASSWORD "your-password"
#define HOSTNAME "Hackathon-Team-04"
#define GUARDIAN_BRIDGE_IP IP_ADDRESS(192, 168, 104, 10)
```

Start from `app/guardian/cloud_config.example.h`. On the day of the demo, first
connect the PC to the same Hackathon Wi-Fi as the AZ3166, then run
`scripts/prepare-guardian.ps1`. It discovers the PC's current Wi-Fi IPv4
address, updates the ignored local config and builds the firmware. Do not set
the board's destination to a WSL or Docker virtual-interface address. The
campaign bridge listens on UDP port `30502`.

After preparing the firmware, explicitly deploy the guardian configuration:
```powershell
.\scripts\deploy.ps1 -Config guardian -Destination D:
```
The guardian image is `build/guardian/app/mxchip_threadx.bin`. Scenario verdicts
are shown while the campaign is running; when the host reports completion or an
error, the OLED resumes the sensor pages.

Run the bridge in WSL with Python 3.8 or newer, Rust/Cargo, and Docker
available. Point it at the root of the cloned `ota-outlaws` repository:

```powershell
wsl.exe -e bash -lc 'cd /mnt/c/path/to/samplex/MXChip/AZ3166 && python3 host/campaign_bridge.py --campaign-dir /home/your-user/ota-outlaws'
```

When Button A is pressed, the bridge runs the OTA Outlaws full campaign
(`campaign run --all`) and immediately forwards each scenario verdict printed
by the running campaign to the board over UDP. The OLED displays each scenario
name and result (for example, `counter_stuck` and `PASS`) as soon as that
scenario completes; it does not wait for the campaign's final report.
Campaign evidence remains in the `runs/` directory of the `ota-outlaws`
repository. This runs the campaign's built-in trace-based tests; the board's
HTS221 samples are not inputs to those tests.

In a separate, non-admin PowerShell window, run
`scripts/start-guardian-udp-relay.ps1` and leave that window open. It forwards
UDP requests from the PC's Wi-Fi address into WSL and campaign results back to
the board. The relay requires a one-time inbound firewall rule, which must be
added from Administrator PowerShell:

```powershell
New-NetFirewallRule -DisplayName "AZ3166 OTA Outlaws UDP 30502" -Direction Inbound -Protocol UDP -LocalPort 30502 -Action Allow -Profile Public -RemoteAddress LocalSubnet
```

The bridge only accepts a campaign request; it does not accept shell commands
or scenario paths from the board. Button B continues to send its existing
one-shot bad-sample event, which is separate from the campaign bridge.

To deploy, use `deploy.ps1` (adjust the destination drive as needed):
```powershell
.\scripts\deploy.ps1 -Config starter -Destination D:
```

### Linux / MacOS (Bash)
Use `build.sh` with the configuration name as the first argument.
```bash
./scripts/build.sh starter
```

To deploy, use `deploy.sh` (adjust the destination path as needed):
```bash
./scripts/deploy.sh /Volumes/AZ3166
```
Pass the build configuration as the second argument when deploying another
configuration, for example `./scripts/deploy.sh /Volumes/AZ3166 guardian`.

To deploy your code on the AZ3166, just plug the board on your computer. When you do so, this will create a virtual drive and a serial port over USB.

Once compilation is finished, you will find the executable in the `MXChip/AZ3166/build/<config>/app` folder (for example, `build/guardian/app/mxchip_threadx.bin`). Deploy the matching configuration to the virtual drive; the AZ3166 boot loader will reset the board and execute it.

You can use any terminal application to connect to the serial port and monitor your application's output. Personally, we use Tera Term, which you can install using `winget`. Just make sure you set the baud rate to **115,200**. On MacOS, we used [SerialTools](https://apps.apple.com/de/app/serialtools/id611021963?mt=12) which you can install via the Mac App Store.

If you deployed this application without any changes, you will get the following output in your terminal:

> ```
> Scanning I2C bus
> ..........................0x1a...0x1e.0x20...........................0x3c...............................0x5c..0x5f..........0x6a.....................
>
> Starting Eclipse ThreadX thread
> 
>
> Initializing WiFi
> ERROR: wifi_ssid is empty
> ERROR: wifi_init (0x00000043)
> ERROR: Failed to initialize the network (0x00000043)
> ```

### WiFi configuration
To connect the board to a WiFi network, edit the following constants found in `cloud_config.h`:

- `HOSTNAME`
- `WIFI_SSID`
- `WIFI_PASSWORD`

Make sure to select an appropriate value for `WIFI_MODE` as well.

If the WiFi is properly congiured, you will get the output below at application startup:

> ```
> Initializing WiFi
>       MAC address: C8:93:46:88:6F:9E
> SUCCESS: WiFi initialized
> ```

## Where to go from here
The application supports multiple configurations that you can select at build time:

- `starter`: Initiates the board and WiFi connectivity.
- `telemetry`: Adds code to read the on-board sensors and print the output.
- `mqtt`: Adds code to publish the telemetry over MQTT. Also creates a second thread subscribing to an MQTT topic; the received messages will be printed. (Note: This configuration builds upon `telemetry`).
- `arcade`: A collection of arcade games. Thanks to Sébastien Heurtematte for this contribution!


### Networking support
NetX Duo is a comprehensive TCP/IPv4 and v6 network stack. It offers built-in HTTP and MQTT clients, among other things.

If you need a local MQTT broker for testing, we recommend using Eclipse Mosquitto. Here is how to install it.

You will find the Eclipse ThreadX documentation here: [https://github.com/eclipse-threadx/rtos-docs](https://github.com/eclipse-threadx/rtos-docs)

**Ubuntu**
```
apt install mosquitto mosquitto mosquitto-clients
```

The mosquitto-clients package installs the `mosquitto_pub`and `mosquitto_sub` utilities.

**Windows**
```
winget install --id=EclipseFoundation.Mosquitto
```

The Windows installer provides the `mosquitto_pub`and `mosquitto_sub` utilities. The default installation directory is `Program Files\mosquittoC:\Program Files\mosquitto`.
