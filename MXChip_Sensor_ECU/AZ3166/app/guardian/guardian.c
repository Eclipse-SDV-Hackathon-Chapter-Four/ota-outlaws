/*
 * SPDX-License-Identifier: MIT
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
#define GUARDIAN_BUTTON_DEBOUNCE_POLLS 3U
#define GUARDIAN_BAD_TEMPERATURE_C     120U
#define GUARDIAN_DATAGRAM_MAX_LENGTH   128U

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

static UINT send_button_event(const CHAR* type, uint32_t sequence)
{
    CHAR message[GUARDIAN_DATAGRAM_MAX_LENGTH];
    int message_length;

    if (strcmp(type, "can_fault") == 0)
    {
        message_length = snprintf(message, sizeof(message),
                                  "{\"type\":\"can_fault\",\"scenario\":\"all\"}");
    }
    else
    {
        message_length = snprintf(message, sizeof(message),
                                  "{\"type\":\"bad_sample\",\"seq\":%lu,\"temperature_c\":%u}",
                                  (unsigned long)sequence,
                                  (unsigned)GUARDIAN_BAD_TEMPERATURE_C);
    }

    if (message_length < 0 || (UINT)message_length >= sizeof(message))
    {
        printf("ERROR: Guardian button event did not fit UDP message buffer\r\n");
        return NX_SIZE_ERROR;
    }

    return send_json(message, (UINT)message_length);
}

void guardian_thread_entry(ULONG parameter)
{
    UINT status;
    uint32_t sequence = 1U;
    ULONG last_sample_tick;
    ULONG last_display_tick;
    ULONG button_status_tick = 0U;
    UINT button_status_active = 0U;
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
        ULONG now = tx_time_get();
        if (button_pressed(&button_a, BUTTON_A_IS_PRESSED))
        {
            status = send_button_event("can_fault", sequence++);
            screen_print_button_status('A', "CAN faults", "4 faults / 5s gaps",
                                       GUARDIAN_BRIDGE_IP, status == NX_SUCCESS);
            button_status_tick = now;
            button_status_active = 1U;
            printf("Button A: request all campaign CAN fault traces; UDP status 0x%08x\r\n",
                   status);
        }
        if (button_pressed(&button_b, BUTTON_B_IS_PRESSED))
        {
            status = send_button_event("bad_sample", sequence++);
            screen_print_button_status('B', "bad temp", "120 C sample",
                                       GUARDIAN_BRIDGE_IP, status == NX_SUCCESS);
            button_status_tick = now;
            button_status_active = 1U;
            printf("Button B: request one-shot %u C sample; UDP status 0x%08x\r\n",
                   (unsigned)GUARDIAN_BAD_TEMPERATURE_C, status);
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
            if (button_status_active &&
                (ULONG)(now - button_status_tick) < GUARDIAN_BUTTON_STATUS_TICKS)
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
