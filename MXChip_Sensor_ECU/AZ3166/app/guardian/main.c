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
 
#include <stdio.h>

#include "board_init.h"
#include "cmsis_utils.h"
#include "tx_api.h"

#define GUARDIAN_THREAD_STACK_SIZE 4096
#define GUARDIAN_THREAD_PRIORITY   4

static TX_THREAD guardian_thread;
static ULONG guardian_thread_stack[GUARDIAN_THREAD_STACK_SIZE / sizeof(ULONG)];

extern void guardian_thread_entry(ULONG parameter);

void tx_application_define(void* first_unused_memory)
{
    UINT status;

    (void)first_unused_memory;
    systick_interval_set(TX_TIMER_TICKS_PER_SECOND);
    status = tx_thread_create(&guardian_thread, "Guardian sensor publisher",
                              guardian_thread_entry, 0, guardian_thread_stack,
                              GUARDIAN_THREAD_STACK_SIZE, GUARDIAN_THREAD_PRIORITY,
                              GUARDIAN_THREAD_PRIORITY, TX_NO_TIME_SLICE, TX_AUTO_START);
    if (status != TX_SUCCESS)
    {
        printf("ERROR: Guardian thread creation failed (0x%08x)\r\n", status);
    }
}

int main(void)
{
    board_init();
    tx_kernel_enter();
    return 0;
}
