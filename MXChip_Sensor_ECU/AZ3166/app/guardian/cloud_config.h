/* 
 * Copyright (c) Microsoft
 * Copyright (c) 2024 Eclipse Foundation
 * 
 *  This program and the accompanying materials are made available 
 *  under the terms of the MIT license which is available at
 *  https://opensource.org/license/mit.
 * 
 *  SPDX-License-Identifier: MIT
 * 
 *  Contributors: 
 *     Microsoft         - Initial version
 *     Frédéric Desbiens - 2024 version.
 * 	   Uttarkar Sopan - Feature enhancements and maintenance
 *     Microsoft Copilot - AI-assisted modifications
 */

#ifndef _GUARDIAN_CLOUD_CONFIG_H
#define _GUARDIAN_CLOUD_CONFIG_H

#include "nx_api.h"

typedef enum
{
    None         = 0,
    WEP          = 1,
    WPA_PSK_TKIP = 2,
    WPA2_PSK_AES = 3
} WiFi_Mode;

#if defined(__has_include)
#if __has_include("cloud_config.local.h")
#include "cloud_config.local.h"
#endif
#endif

#ifndef HOSTNAME
#define HOSTNAME "Hackathon-Team-04"
#endif
#ifndef WIFI_SSID
#define WIFI_SSID ""
#endif
#ifndef WIFI_PASSWORD
#define WIFI_PASSWORD ""
#endif
#ifndef GUARDIAN_BRIDGE_IP
#define GUARDIAN_BRIDGE_IP IP_ADDRESS(0, 0, 0, 0)
#endif

#define WIFI_MODE WPA2_PSK_AES

#endif
