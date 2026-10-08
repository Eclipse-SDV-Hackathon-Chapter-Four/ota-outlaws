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
 *     Uttarkar Sopan    - Feature enhanced to communicate with Host via IP.
 *     Microsoft Copilot - AI-assisted modifications
 */

#ifndef _CLOUD_CONFIG_H
#define _CLOUD_CONFIG_H

#include "nx_api.h"

typedef enum
{
    None         = 0,
    WEP          = 1,
    WPA_PSK_TKIP = 2,
    WPA2_PSK_AES = 3
} WiFi_Mode;

// ----------------------------------------------------------------------------
// WiFi connection config
// ----------------------------------------------------------------------------
#if defined(__has_include)
#if __has_include("cloud_config.local.h")
#include "cloud_config.local.h"
#endif
#endif

#ifndef HOSTNAME
#define HOSTNAME "eclipse-threadx"
#endif
#ifndef WIFI_SSID
#define WIFI_SSID ""
#endif
#ifndef WIFI_PASSWORD
#define WIFI_PASSWORD ""
#endif
#define WIFI_MODE     WPA2_PSK_AES

// ----------------------------------------------------------------------------
// MQTT Config
// ----------------------------------------------------------------------------
#define MQTT_CLIENT_NAME     "ThreadXAZ3166" //Change to unique name.
// Use test.mosquitto.org in a pinch.
#define MQTT_LOCAL_BROKER_IP (IP_ADDRESS(5, 196, 78, 28))
#define MQTT_SUBSCRIBE_TOPIC MQTT_CLIENT_NAME "/incoming" 
#define MQTT_PUBLISH_TOPIC   MQTT_CLIENT_NAME "/telemetry" 

// ----------------------------------------------------------------------------
// MQTT Support infrastructure
// ----------------------------------------------------------------------------
extern TX_QUEUE mqtt_queue;
extern TX_EVENT_FLAGS_GROUP mqtt_app_flag;
#define MQTT_RECEIVE_EVENT 1
#define MQTT_MESSAGE_READY 2
#define MQTT_ALL_EVENTS    3


#endif // _CLOUD_CONFIG_H
