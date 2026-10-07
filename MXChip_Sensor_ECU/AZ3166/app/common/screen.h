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

#ifndef _SCREEN_H
#define _SCREEN_H

#include <stddef.h>
#include <stdint.h>
#include "sensor.h"

/* Enumration for line on the screen */
typedef enum
{
    L0 = 0,
    L1 = 18,
    L2 = 36,
    L3 = 54
} LINE_NUM;

void screen_print(char* str, LINE_NUM line);
void screen_printn(const char* str, unsigned int str_length, LINE_NUM line);
void screen_print_sensor_page(unsigned int page, hts221_data_t hts221_data,
                              lps22hb_t lps22hb_data, lsm6dsl_data_t lsm6dsl_data,
                              lis2mdl_data_t lis2mdl_data);
void screen_print_button_status(char button, const char* action, const char* detail,
                                uint32_t destination_ip, int send_succeeded);

#endif // _SCREEN_H