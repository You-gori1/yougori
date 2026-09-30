/* SPDX-License-Identifier: BSD-2-Clause */
#include <stdint.h>
#include <string.h>
#include "Tpm.h"
#include "Platform.h"
#include "tpm-api.h"

#define OD_EXPORT __declspec(dllexport)
int od_nv_open(const char *, int);
void od_nv_close(void);
static int opened;

OD_EXPORT uint32_t od_tpm_version(void) { return OD_TPM_ABI; }

OD_EXPORT void od_tpm_close(void)
{
    if (opened) _plat__Signal_PowerOff();
    od_nv_close();
    opened = 0;
}

OD_EXPORT int od_tpm_open(const char *path, int create)
{
    if (opened || od_nv_open(path, create) != 0) return -1;
    if (_plat__NVEnable(NULL, 0) != 0
        || (create && TPM_Manufacture(MANUF_FIRST_TIME) != MANUF_OK)) {
        od_tpm_close();
        return -1;
    }
    opened = 1;
    return 0;
}

OD_EXPORT int od_tpm_reset(void)
{
    if (!opened || _plat__NVEnable(NULL, 0) != 0) return -1;
    _plat__Signal_PowerOn();
    _plat__Signal_Reset();
    _plat__SetNvAvail();
    return _plat__GetNvReadyState() == NV_READY ? 0 : -1;
}

OD_EXPORT int od_tpm_execute(uint8_t locality, const uint8_t *input, uint32_t input_size,
                             uint8_t *output, uint32_t *output_size)
{
    if (!opened || locality > 4 || !input || !output || !output_size
        || input_size < 10 || input_size > OD_TPM_BUFFER || *output_size < 10) return -1;
    uint32_t declared = ((uint32_t)input[2] << 24) | ((uint32_t)input[3] << 16)
                      | ((uint32_t)input[4] << 8) | input[5];
    if (declared != input_size) return -1;
    /* The core can use and modify its request and response storage. Never let
     * it retain a pointer into QEMU guest memory, or overwrite a short buffer. */
    uint8_t request[OD_TPM_BUFFER], response[OD_TPM_BUFFER];
    uint8_t *response_ptr = response;
    uint32_t response_size = sizeof(response);
    memcpy(request, input, input_size);
    _plat__ClearCancel();
    _plat__LocalitySet(locality);
    _plat__RunCommand(input_size, request, &response_size, &response_ptr);
    if (response_size < 10 || response_size > *output_size || response_size > sizeof(response)) return -1;
    memcpy(output, response_ptr, response_size);
    *output_size = response_size;
    return 0;
}
