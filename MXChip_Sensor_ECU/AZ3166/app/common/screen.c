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

#include "screen.h"

#include "nanoprintf.h"
#include "ssd1306.h"

void screen_print(char* str, LINE_NUM line)
{
    ssd1306_Fill(Black);
    ssd1306_SetCursor(2, line);
    ssd1306_WriteString(str, Font_11x18, White);
    ssd1306_UpdateScreen();
}

void screen_printn(const char* str, unsigned int str_length, LINE_NUM line)
{
    ssd1306_Fill(Black);
    ssd1306_SetCursor(2, line);

    for (unsigned int i = 0; i < str_length; ++i)
    {
        if (ssd1306_WriteChar(str[i], Font_11x18, White) != str[i])
        {
            return;
        }
    }

    ssd1306_UpdateScreen();
}

static void screen_print_header(void)
{
    static char header_line_1[] = "ECLIPSE SDV";
    static char header_line_2[] = "HACKATHON";

    ssd1306_SetCursor(1, 0);
    ssd1306_WriteString(header_line_1, Font_6x8, White);
    ssd1306_SetCursor(2, 0);
    ssd1306_WriteString(header_line_1, Font_6x8, White);
    ssd1306_SetCursor(1, 8);
    ssd1306_WriteString(header_line_2, Font_6x8, White);
    ssd1306_SetCursor(2, 8);
    ssd1306_WriteString(header_line_2, Font_6x8, White);
}

void screen_print_sensor_page(unsigned int page, hts221_data_t hts221_data,
                              lps22hb_t lps22hb_data, lsm6dsl_data_t lsm6dsl_data,
                              lis2mdl_data_t lis2mdl_data)
{
    char lines[6][20] = {{0}};

    switch (page)
    {
        case 0:
            npf_snprintf(lines[0], sizeof(lines[0]), "HTS221 TEMP/RH");
            npf_snprintf(lines[1], sizeof(lines[1]), "T: %.2f C",
                         (double)hts221_data.temperature_degC);
            npf_snprintf(lines[2], sizeof(lines[2]), "Humidity: %.2f%%",
                         (double)hts221_data.humidity_perc);
            break;
        case 1:
            npf_snprintf(lines[0], sizeof(lines[0]), "LPS22HB PRESSURE");
            npf_snprintf(lines[1], sizeof(lines[1]), "P: %.2f hPa",
                         (double)lps22hb_data.pressure_hPa);
            npf_snprintf(lines[2], sizeof(lines[2]), "T: %.2f C",
                         (double)lps22hb_data.temperature_degC);
            break;
        case 2:
            npf_snprintf(lines[0], sizeof(lines[0]), "LSM6DSL ACCEL mg");
            npf_snprintf(lines[1], sizeof(lines[1]), "X: %+.1f",
                         (double)lsm6dsl_data.acceleration_mg[0]);
            npf_snprintf(lines[2], sizeof(lines[2]), "Y: %+.1f",
                         (double)lsm6dsl_data.acceleration_mg[1]);
            npf_snprintf(lines[3], sizeof(lines[3]), "Z: %+.1f",
                         (double)lsm6dsl_data.acceleration_mg[2]);
            npf_snprintf(lines[4], sizeof(lines[4]), "T: %.2f C",
                         (double)lsm6dsl_data.temperature_degC);
            break;
        case 3:
            npf_snprintf(lines[0], sizeof(lines[0]), "LSM6DSL GYRO mdps");
            npf_snprintf(lines[1], sizeof(lines[1]), "X: %+.0f",
                         (double)lsm6dsl_data.angular_rate_mdps[0]);
            npf_snprintf(lines[2], sizeof(lines[2]), "Y: %+.0f",
                         (double)lsm6dsl_data.angular_rate_mdps[1]);
            npf_snprintf(lines[3], sizeof(lines[3]), "Z: %+.0f",
                         (double)lsm6dsl_data.angular_rate_mdps[2]);
            break;
        default:
            npf_snprintf(lines[0], sizeof(lines[0]), "LIS2MDL MAG mG");
            npf_snprintf(lines[1], sizeof(lines[1]), "X: %+.1f",
                         (double)lis2mdl_data.magnetic_mG[0]);
            npf_snprintf(lines[2], sizeof(lines[2]), "Y: %+.1f",
                         (double)lis2mdl_data.magnetic_mG[1]);
            npf_snprintf(lines[3], sizeof(lines[3]), "Z: %+.1f",
                         (double)lis2mdl_data.magnetic_mG[2]);
            npf_snprintf(lines[4], sizeof(lines[4]), "T: %.2f C",
                         (double)lis2mdl_data.temperature_degC);
            break;
    }

    ssd1306_Fill(Black);
    screen_print_header();

    for (size_t i = 0; i < 6; ++i)
    {
        ssd1306_SetCursor(2, (uint8_t)(16 + i * 8));
        ssd1306_WriteString(lines[i], Font_6x8, White);
    }

    ssd1306_UpdateScreen();
}

void screen_print_button_status(char button, const char* action, const char* detail,
                                uint32_t destination_ip, int send_succeeded)
{
    char lines[6][22] = {{0}};

    npf_snprintf(lines[0], sizeof(lines[0]), "BUTTON %c PRESSED", button);
    npf_snprintf(lines[1], sizeof(lines[1]), "Sending %s", action);
    npf_snprintf(lines[2], sizeof(lines[2]), "%s", detail);
    npf_snprintf(lines[3], sizeof(lines[3]), "To %u.%u.%u.%u",
                 (unsigned)((destination_ip >> 24) & 0xffU),
                 (unsigned)((destination_ip >> 16) & 0xffU),
                 (unsigned)((destination_ip >> 8) & 0xffU),
                 (unsigned)(destination_ip & 0xffU));
    npf_snprintf(lines[4], sizeof(lines[4]), "UDP 30502");
    npf_snprintf(lines[5], sizeof(lines[5]), "%s",
                 send_succeeded ? "Send status: OK" : "Send status: FAIL");

    ssd1306_Fill(Black);
    screen_print_header();

    for (size_t i = 0; i < 6; ++i)
    {
        ssd1306_SetCursor(2, (uint8_t)(16 + i * 8));
        ssd1306_WriteString(lines[i], Font_6x8, White);
    }

    ssd1306_UpdateScreen();
}