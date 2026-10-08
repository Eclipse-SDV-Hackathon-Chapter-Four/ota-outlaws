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
 *     Uttarkar Sopan    - Feature enhanced to display the fault injected log via button pressed.
 *     Microsoft Copilot - AI-assisted modifications
 */
 
#include "cloud_config.h"
#include "board_init.h"
#include "nx_api.h"
#include "screen.h"
#include "sensor.h"
#include "wwd_networking.h"

#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#define GUARDIAN_UDP_PORT              30502U
#define GUARDIAN_SAMPLE_INTERVAL_TICKS ((TX_TIMER_TICKS_PER_SECOND + 9U) / 10U)
#define GUARDIAN_DISPLAY_INTERVAL_TICKS TX_TIMER_TICKS_PER_SECOND
#define GUARDIAN_POLL_INTERVAL_TICKS   ((TX_TIMER_TICKS_PER_SECOND + 9U) / 10U)
#define GUARDIAN_BUTTON_STATUS_TICKS   (TX_TIMER_TICKS_PER_SECOND * 3U)
#define GUARDIAN_CAMPAIGN_ACK_TICKS    (TX_TIMER_TICKS_PER_SECOND * 10U)
#define GUARDIAN_BUTTON_DEBOUNCE_POLLS 3U
#define GUARDIAN_BAD_TEMPERATURE_C     120U
#define GUARDIAN_DATAGRAM_MAX_LENGTH   160U
#define GUARDIAN_SCENARIO_MAX_LENGTH   63U
#define GUARDIAN_VERDICT_MAX_LENGTH    15U

typedef struct
{
    UINT candidate_pressed;
    UINT stable_pressed;
    UINT stable_polls;
} button_debounce_t;

static NX_UDP_SOCKET guardian_socket;

static UINT button_pressed(button_debounce_t* state, UINT is_pressed)
{
    if (is_pressed != state->candidate_pressed)
    {
        state->candidate_pressed = is_pressed;
        state->stable_polls = 1U;
        return 0U;
    }

    if (state->stable_polls < GUARDIAN_BUTTON_DEBOUNCE_POLLS)
    {
        state->stable_polls++;
    }

    if (state->stable_polls == GUARDIAN_BUTTON_DEBOUNCE_POLLS &&
        state->stable_pressed != state->candidate_pressed)
    {
        state->stable_pressed = state->candidate_pressed;
        return state->stable_pressed;
    }

    return 0U;
}

static UINT send_json(const CHAR* message, UINT message_length)
{
    NX_PACKET* packet = NX_NULL;
    NXD_ADDRESS host_address;
    UINT status;

    status = nx_packet_allocate(&nx_pool[0], &packet, NX_UDP_PACKET, NX_WAIT_FOREVER);
    if (status != NX_SUCCESS)
    {
        printf("ERROR: Guardian UDP packet allocation failed (0x%08x)\r\n", status);
        return status;
    }

    status = nx_packet_data_append(packet, (VOID*)message, message_length,
                                   &nx_pool[0], NX_WAIT_FOREVER);
    if (status != NX_SUCCESS)
    {
        printf("ERROR: Guardian UDP packet assembly failed (0x%08x)\r\n", status);
        nx_packet_release(packet);
        return status;
    }

    memset(&host_address, 0, sizeof(host_address));
    host_address.nxd_ip_version = 4U;
    host_address.nxd_ip_address.v4 = GUARDIAN_BRIDGE_IP;
    status = nxd_udp_socket_send(&guardian_socket, packet, &host_address, GUARDIAN_UDP_PORT);
    if (status != NX_SUCCESS)
    {
        printf("ERROR: Guardian UDP send failed (0x%08x)\r\n", status);
        nx_packet_release(packet);
    }

    return status;
}

static UINT send_sample(uint32_t sequence, float temperature_celsius)
{
    CHAR message[GUARDIAN_DATAGRAM_MAX_LENGTH];
    int32_t temperature_hundredths;
    int32_t fraction;
    int message_length;

    if (!isfinite(temperature_celsius))
    {
        printf("ERROR: Refusing to send non-finite HTS221 temperature\r\n");
        return NX_INVALID_PARAMETERS;
    }

    temperature_hundredths = (int32_t)(temperature_celsius * 100.0f);
    fraction = temperature_hundredths % 100;
    if (fraction < 0)
    {
        fraction = -fraction;
    }
    message_length = snprintf(message, sizeof(message),
                              "{\"type\":\"sample\",\"seq\":%lu,\"temperature_c\":%d.%02d}",
                              (unsigned long)sequence,
                              (int)(temperature_hundredths / 100), (int)fraction);
    if (message_length < 0 || (UINT)message_length >= sizeof(message))
    {
        printf("ERROR: Guardian sample did not fit UDP message buffer\r\n");
        return NX_SIZE_ERROR;
    }

    return send_json(message, (UINT)message_length);
}

static UINT send_campaign_request(uint32_t request_id)
{
    CHAR message[GUARDIAN_DATAGRAM_MAX_LENGTH];
    int message_length;

    message_length = snprintf(message, sizeof(message),
                              "{\"type\":\"campaign\",\"id\":%lu}",
                              (unsigned long)request_id);

    if (message_length < 0 || (UINT)message_length >= sizeof(message))
    {
        printf("ERROR: Guardian campaign request did not fit UDP message buffer\r\n");
        return NX_SIZE_ERROR;
    }

    return send_json(message, (UINT)message_length);
}

static UINT send_bad_sample(uint32_t sequence)
{
    CHAR message[GUARDIAN_DATAGRAM_MAX_LENGTH];
    int message_length = snprintf(message, sizeof(message),
                                  "{\"type\":\"bad_sample\",\"seq\":%lu}",
                                  (unsigned long)sequence);

    if (message_length < 0 || (UINT)message_length >= sizeof(message))
    {
        printf("ERROR: Guardian button event did not fit UDP message buffer\r\n");
        return NX_SIZE_ERROR;
    }

    return send_json(message, (UINT)message_length);
}

static UINT json_string_field(const CHAR* message, const CHAR* name,
                              CHAR* value, size_t value_size)
{
    CHAR key[32];
    const CHAR* cursor;
    size_t length = 0U;
    int key_length = snprintf(key, sizeof(key), "\"%s\"", name);

    if (key_length < 0 || (size_t)key_length >= sizeof(key))
    {
        return 0U;
    }

    cursor = strstr(message, key);
    if (cursor == NX_NULL)
    {
        return 0U;
    }
    cursor += key_length;
    while (*cursor == ' ' || *cursor == '\t')
    {
        cursor++;
    }
    if (*cursor++ != ':')
    {
        return 0U;
    }
    while (*cursor == ' ' || *cursor == '\t')
    {
        cursor++;
    }
    if (*cursor++ != '"')
    {
        return 0U;
    }

    while (*cursor != '\0' && *cursor != '"')
    {
        CHAR character = *cursor++;
        if (!((character >= 'a' && character <= 'z') ||
              (character >= 'A' && character <= 'Z') ||
              (character >= '0' && character <= '9') ||
              character == '_' || character == '-'))
        {
            return 0U;
        }
        if (length + 1U < value_size)
        {
            value[length++] = character;
        }
    }

    if (*cursor != '"' || length == 0U)
    {
        return 0U;
    }
    value[length] = '\0';
    return 1U;
}

static UINT json_uint_field(const CHAR* message, const CHAR* name, uint32_t* value)
{
    CHAR key[32];
    const CHAR* cursor;
    uint32_t parsed = 0U;
    int key_length = snprintf(key, sizeof(key), "\"%s\"", name);

    if (key_length < 0 || (size_t)key_length >= sizeof(key))
    {
        return 0U;
    }

    cursor = strstr(message, key);
    if (cursor == NX_NULL)
    {
        return 0U;
    }
    cursor += key_length;
    while (*cursor == ' ' || *cursor == '\t')
    {
        cursor++;
    }
    if (*cursor++ != ':')
    {
        return 0U;
    }
    while (*cursor == ' ' || *cursor == '\t')
    {
        cursor++;
    }
    if (*cursor < '0' || *cursor > '9')
    {
        return 0U;
    }

    do
    {
        uint32_t digit = (uint32_t)(*cursor - '0');
        if (parsed > (UINT32_MAX - digit) / 10U)
        {
            return 0U;
        }
        parsed = parsed * 10U + digit;
        cursor++;
    } while (*cursor >= '0' && *cursor <= '9');

    *value = parsed;
    return 1U;
}

static UINT receive_campaign_result(uint32_t expected_request_id,
                                    CHAR scenario[GUARDIAN_SCENARIO_MAX_LENGTH + 1U],
                                    CHAR verdict[GUARDIAN_VERDICT_MAX_LENGTH + 1U],
                                    uint32_t* result_index, uint32_t* result_total,
                                    UINT* request_acknowledged, UINT* result_received,
                                    UINT* campaign_finished, UINT* campaign_failed)
{
    NX_PACKET* packet = NX_NULL;
    CHAR message[GUARDIAN_DATAGRAM_MAX_LENGTH];
    ULONG packet_length = 0U;
    ULONG bytes_copied = 0U;
    unsigned long response_id = 0UL;
    UINT status;

    *request_acknowledged = 0U;
    *result_received = 0U;
    *campaign_finished = 0U;
    *campaign_failed = 0U;
    *result_index = 0U;
    *result_total = 0U;

    status = nx_udp_socket_receive(&guardian_socket, &packet, TX_NO_WAIT);
    if (status == NX_NO_PACKET)
    {
        return NX_SUCCESS;
    }
    if (status != NX_SUCCESS)
    {
        return status;
    }

    status = nx_packet_length_get(packet, &packet_length);
    if (status != NX_SUCCESS || packet_length >= sizeof(message))
    {
        nx_packet_release(packet);
        printf("ERROR: Guardian campaign response is invalid or too large\r\n");
        return status == NX_SUCCESS ? NX_SIZE_ERROR : status;
    }

    status = nx_packet_data_retrieve(packet, message, &bytes_copied);
    nx_packet_release(packet);
    if (status != NX_SUCCESS || bytes_copied != packet_length)
    {
        printf("ERROR: Guardian campaign response read failed (0x%08x)\r\n", status);
        return status == NX_SUCCESS ? NX_SIZE_ERROR : status;
    }
    message[bytes_copied] = '\0';

    if (sscanf(message, "{\"type\":\"campaign_ack\",\"id\":%lu}", &response_id) == 1 &&
        response_id == (unsigned long)expected_request_id)
    {
        *request_acknowledged = 1U;
        return NX_SUCCESS;
    }

    if (sscanf(message,
               "{\"type\":\"campaign_result\",\"id\":%lu",
               &response_id) == 1 &&
        json_string_field(message, "scenario", scenario,
                          GUARDIAN_SCENARIO_MAX_LENGTH + 1U) &&
        json_string_field(message, "verdict", verdict,
                          GUARDIAN_VERDICT_MAX_LENGTH + 1U) &&
        response_id == (unsigned long)expected_request_id)
    {
        (void)json_uint_field(message, "n", result_index);
        (void)json_uint_field(message, "total", result_total);
        if (*result_index == 0U || *result_total == 0U ||
            *result_index > *result_total)
        {
            *result_index = 0U;
            *result_total = 0U;
        }
        *request_acknowledged = 1U;
        *result_received = 1U;
        return NX_SUCCESS;
    }

    if (sscanf(message, "{\"type\":\"campaign_error\",\"id\":%lu}", &response_id) == 1 &&
        response_id == (unsigned long)expected_request_id)
    {
        *campaign_finished = 1U;
        *campaign_failed = 1U;
        return NX_SUCCESS;
    }

    if (sscanf(message, "{\"type\":\"campaign_complete\",\"id\":%lu}", &response_id) == 1 &&
        response_id == (unsigned long)expected_request_id)
    {
        *campaign_finished = 1U;
    }

    return NX_SUCCESS;
}

void guardian_thread_entry(ULONG parameter)
{
    UINT status;
    uint32_t sequence = 1U;
    uint32_t campaign_request_id = 0U;
    ULONG last_sample_tick;
    ULONG last_display_tick;
    ULONG button_status_tick = 0U;
    ULONG campaign_request_tick = 0U;
    UINT button_status_active = 0U;
    UINT campaign_running = 0U;
    UINT campaign_acknowledged = 0U;
    UINT campaign_result_visible = 0U;
    ULONG poll_ticks = GUARDIAN_POLL_INTERVAL_TICKS;
    button_debounce_t button_a = {0U, 0U, 0U};
    button_debounce_t button_b = {0U, 0U, 0U};
    (void)parameter;

    if (poll_ticks == 0U)
    {
        poll_ticks = 1U;
    }

    printf("Starting AZ3166 OTA Outlaws sensor publisher\r\n");

    if (WIFI_SSID[0] == '\0' || WIFI_PASSWORD[0] == '\0')
    {
        printf("ERROR: Set WIFI_SSID and WIFI_PASSWORD in cloud_config.local.h\r\n");
        return;
    }
    if (GUARDIAN_BRIDGE_IP == IP_ADDRESS(0, 0, 0, 0))
    {
        printf("ERROR: Set GUARDIAN_BRIDGE_IP to the reachable host adapter IP\r\n");
        return;
    }

    status = wwd_network_init((CHAR*)WIFI_SSID, (CHAR*)WIFI_PASSWORD, WIFI_MODE);
    if (status != NX_SUCCESS)
    {
        printf("ERROR: Wi-Fi/network initialization failed (0x%08x)\r\n", status);
        return;
    }

    status = wwd_network_connect();
    if (status != NX_SUCCESS)
    {
        printf("ERROR: Wi-Fi connection failed (0x%08x)\r\n", status);
        return;
    }

    status = nx_udp_socket_create(&nx_ip, &guardian_socket, "Guardian host adapter",
                                  NX_IP_NORMAL, NX_FRAGMENT_OKAY, NX_IP_TIME_TO_LIVE, 4U);
    if (status != NX_SUCCESS)
    {
        printf("ERROR: Guardian UDP socket creation failed (0x%08x)\r\n", status);
        return;
    }

    status = nx_udp_socket_bind(&guardian_socket, NX_ANY_PORT, NX_WAIT_FOREVER);
    if (status != NX_SUCCESS)
    {
        printf("ERROR: Guardian UDP socket bind failed (0x%08x)\r\n", status);
        nx_udp_socket_delete(&guardian_socket);
        return;
    }

    printf("Host adapter at IPv4 %u.%u.%u.%u UDP %u\r\n",
           (unsigned)((GUARDIAN_BRIDGE_IP >> 24) & 0xffU),
           (unsigned)((GUARDIAN_BRIDGE_IP >> 16) & 0xffU),
           (unsigned)((GUARDIAN_BRIDGE_IP >> 8) & 0xffU),
           (unsigned)(GUARDIAN_BRIDGE_IP & 0xffU),
           (unsigned)GUARDIAN_UDP_PORT);

    last_sample_tick = tx_time_get();
    last_display_tick = last_sample_tick;
    while (1)
    {
        CHAR scenario[GUARDIAN_SCENARIO_MAX_LENGTH + 1U] = {0};
        CHAR verdict[GUARDIAN_VERDICT_MAX_LENGTH + 1U] = {0};
        uint32_t result_index = 0U;
        uint32_t result_total = 0U;
        UINT request_acknowledged = 0U;
        UINT result_received = 0U;
        UINT campaign_finished = 0U;
        UINT campaign_failed = 0U;
        ULONG now = tx_time_get();

        status = receive_campaign_result(campaign_request_id, scenario, verdict,
                                         &result_index, &result_total,
                                         &request_acknowledged, &result_received,
                                         &campaign_finished, &campaign_failed);
        if (status != NX_SUCCESS)
        {
            printf("ERROR: Guardian campaign response receive failed (0x%08x)\r\n", status);
        }
        else
        {
            if (request_acknowledged)
            {
                campaign_acknowledged = 1U;
            }
            if (campaign_failed)
            {
                campaign_running = 0U;
                campaign_result_visible = 0U;
                screen_print_campaign_result("campaign", "ERROR", 0U, 0U);
                button_status_tick = now;
                button_status_active = 1U;
            }
            else if (result_received)
            {
                campaign_result_visible = 1U;
                screen_print_campaign_result(scenario, verdict,
                                             result_index, result_total);
                button_status_tick = now;
                button_status_active = 1U;
                printf("Campaign result: %s %s\r\n", scenario, verdict);
            }
            else if (campaign_finished)
            {
                campaign_running = 0U;
                campaign_result_visible = 0U;
                button_status_active = 0U;
            }
        }

        if (button_pressed(&button_a, BUTTON_A_IS_PRESSED))
        {
            if (campaign_running)
            {
                screen_print_campaign_result("campaign", "BUSY", 0U, 0U);
            }
            else
            {
                campaign_request_id = sequence++;
                status = send_campaign_request(campaign_request_id);
                if (status == NX_SUCCESS)
                {
                    campaign_running = 1U;
                    campaign_acknowledged = 0U;
                    campaign_result_visible = 0U;
                    campaign_request_tick = now;
                    screen_print_campaign_result("campaign", "RUNNING", 0U, 0U);
                    printf("Button A: started full OTA Outlaws campaign (request %lu)\r\n",
                           (unsigned long)campaign_request_id);
                }
                else
                {
                    screen_print_campaign_result("campaign", "SEND ERROR", 0U, 0U);
                    printf("ERROR: Button A campaign request failed (0x%08x)\r\n", status);
                }
            }
            button_status_tick = now;
            button_status_active = 1U;
        }
        if (button_pressed(&button_b, BUTTON_B_IS_PRESSED))
        {
            campaign_result_visible = 0U;
            status = send_bad_sample(sequence++);
            screen_print_button_status('B', "bad temp", "120 C sample",
                                       GUARDIAN_BRIDGE_IP, status == NX_SUCCESS);
            button_status_tick = now;
            button_status_active = 1U;
            printf("Button B: request one-shot %u C sample; UDP status 0x%08x\r\n",
                   (unsigned)GUARDIAN_BAD_TEMPERATURE_C, status);
        }

        if (campaign_running && !campaign_acknowledged &&
            (ULONG)(now - campaign_request_tick) >= GUARDIAN_CAMPAIGN_ACK_TICKS)
        {
            campaign_running = 0U;
            campaign_result_visible = 1U;
            screen_print_campaign_result("campaign", "NO HOST", 0U, 0U);
            button_status_tick = now;
            button_status_active = 1U;
            printf("ERROR: No campaign bridge response within 10 seconds\r\n");
        }

        if ((ULONG)(now - last_sample_tick) >= GUARDIAN_SAMPLE_INTERVAL_TICKS)
        {
            hts221_data_t climate = hts221_data_read();

            status = send_sample(sequence++, climate.temperature_degC);
            if (status != NX_SUCCESS)
            {
                printf("ERROR: HTS221 sample publish failed (0x%08x)\r\n", status);
            }
            last_sample_tick = now;
        }

        if ((ULONG)(now - last_display_tick) >= GUARDIAN_DISPLAY_INTERVAL_TICKS)
        {
            if (campaign_running || campaign_result_visible ||
                (button_status_active &&
                 (ULONG)(now - button_status_tick) < GUARDIAN_BUTTON_STATUS_TICKS))
            {
                last_display_tick = now;
            }
            else
            {
                lps22hb_t pressure = lps22hb_data_read();
                hts221_data_t climate = hts221_data_read();
                lsm6dsl_data_t motion = lsm6dsl_data_read();
                lis2mdl_data_t magnetic = lis2mdl_data_read();
                int32_t temperature_hundredths =
                    (int32_t)(climate.temperature_degC * 100.0f);
                int32_t humidity_hundredths =
                    (int32_t)(climate.humidity_perc * 100.0f);
                int32_t temperature_fraction = temperature_hundredths % 100;
                int32_t humidity_fraction = humidity_hundredths % 100;

                if (temperature_fraction < 0)
                {
                    temperature_fraction = -temperature_fraction;
                }
                if (humidity_fraction < 0)
                {
                    humidity_fraction = -humidity_fraction;
                }

                button_status_active = 0U;
                screen_print_sensor_page((sequence - 1U) % 5U,
                                         climate, pressure, motion, magnetic);
                printf("HTS221 temperature %d.%02d C, humidity %d.%02d%%; sequence %lu\r\n",
                       (int)(temperature_hundredths / 100),
                       (int)temperature_fraction,
                       (int)(humidity_hundredths / 100),
                       (int)humidity_fraction,
                       (unsigned long)(sequence - 1U));
                last_display_tick = now;
            }
        }

        tx_thread_sleep(poll_ticks);
    }
}
