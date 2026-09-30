/* SPDX-License-Identifier: BSD-2-Clause
 * Tests only the bundled software TPM with caller-owned disposable files.
 * Never uses TBS or the physical TPM on the developer's computer.
 */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "tpm-api.h"
static OdTpmExecute execute;
static uint8_t response[OD_TPM_BUFFER];
static uint32_t response_size;
static void require(int ok, const char *what) { if (!ok) { fprintf(stderr, "FAIL: %s\n", what); exit(1); } }
static void command(const char *hex, const char *label)
{
    uint8_t request[OD_TPM_BUFFER];
    size_t size = strlen(hex) / 2;
    require(size <= sizeof(request), "test command bounds");
    for (size_t i = 0; i < size; i++) {
        char byte[3] = {hex[i * 2], hex[i * 2 + 1], 0};
        request[i] = (uint8_t)strtoul(byte, NULL, 16);
    }
    response_size = sizeof(response);
    require(execute(0, request, (uint32_t)size, response, &response_size) == 0, label);
    if (response_size < 10 || memcmp(response + 6, "\0\0\0\0", 4)) {
        fprintf(stderr, "%s response:", label);
        for (unsigned i = 0; i < response_size; i++) fprintf(stderr, "%02x", response[i]);
        fprintf(stderr, "\n"); exit(1);
    }
}
int main(int argc, char **argv)
{
    require(argc == 3, "usage: test-tpm.exe library.dll NEW-disposable-state-file");
    HMODULE library = LoadLibraryA(argv[1]);
    require(library != NULL, "load software TPM library");
    OdTpmVersion version = (OdTpmVersion)(void *)GetProcAddress(library, "od_tpm_version");
    OdTpmOpen open_tpm = (OdTpmOpen)(void *)GetProcAddress(library, "od_tpm_open");
    OdTpmClose close_tpm = (OdTpmClose)(void *)GetProcAddress(library, "od_tpm_close");
    OdTpmReset reset = (OdTpmReset)(void *)GetProcAddress(library, "od_tpm_reset");
    execute = (OdTpmExecute)(void *)GetProcAddress(library, "od_tpm_execute");
    require(version && open_tpm && close_tpm && reset && execute && version() == OD_TPM_ABI, "ABI");
    require(open_tpm(argv[2], 0) != 0, "missing identity is not recreated");
    require(open_tpm(argv[2], 1) == 0, "manufacture NEW identity");
    HANDLE locked = CreateFileA(argv[2], GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE, NULL, OPEN_EXISTING, 0, NULL);
    require(locked == INVALID_HANDLE_VALUE, "TPM state is exclusively locked while running");
    require(reset() == 0, "power on");
    command("80010000000c000001440000", "Startup(CLEAR)");
    command("80010000000b0000014301", "full cryptographic self-test");
    command("8001000000160000017a000000060000010000000010", "TPM properties");
    command("80010000000c0000017b0020", "random 1");
    uint8_t random[32];
    require(response_size == 44, "32 random bytes");
    memcpy(random, response + 12, 32);
    command("80010000000c0000017b0020", "random 2");
    require(memcmp(random, response + 12, 32) != 0, "OS-backed fresh entropy");
    uint8_t invalid[10] = {0x80,1,0xff,0xff,0xff,0xff,0,0,1,0x7b};
    response_size = sizeof(response);
    require(execute(0, invalid, sizeof(invalid), response, &response_size) != 0, "reject inconsistent TPM command length");
    require(execute(5, invalid, sizeof(invalid), response, &response_size) != 0, "reject invalid locality");
    command("80010000000c0000017b0020", "TPM still works after invalid input");
    command("80020000002d0000012a40000001000000094000000900000000000000000e01500020000b0202000200000010", "NV define");
    command("80020000003300000137400000010150002000000009400000090000000000001000112233445566778899aabbccddeeff0000", "NV write");
    command("80010000000c000001450000", "Shutdown(CLEAR)");
    close_tpm();
    require(open_tpm(argv[2], 1) != 0, "existing identity cannot be overwritten");
    require(open_tpm(argv[2], 0) == 0 && reset() == 0, "reopen persisted identity");
    command("80010000000c000001440000", "Startup after reopen");
    command("8002000000230000014e40000001015000200000000940000009000000000000100000", "NV read after reopen");
    const uint8_t expected[] = {0,0x11,0x22,0x33,0x44,0x55,0x66,0x77,0x88,0x99,0xaa,0xbb,0xcc,0xdd,0xee,0xff};
    require(response_size >= 32 && !memcmp(response + 16, expected, 16), "NV data survived restart");
    close_tpm();
    // Both generations now contain the test NV data. Damage the newer slot,
    // like an interrupted commit; the older complete generation must recover.
    FILE *state = fopen(argv[2], "r+b");
    require(state != NULL, "open our disposable state fixture");
    uint64_t sequence[2];
    require(fseek(state, 8, SEEK_SET) == 0 && fread(&sequence[0], 8, 1, state) == 1, "first sequence");
    require(fseek(state, 64+16384+8, SEEK_SET) == 0 && fread(&sequence[1], 8, 1, state) == 1, "second sequence");
    unsigned damaged = sequence[0] > sequence[1] ? 0 : 1;
    require(fseek(state, (long)(damaged * (64+16384)), SEEK_SET) == 0 && fputc(0, state) == 0 && fclose(state) == 0, "simulate torn commit");
    require(open_tpm(argv[2], 0) == 0 && reset() == 0, "recover previous valid generation");
    command("80010000000c000001440000", "Startup after torn commit");
    command("8002000000230000014e40000001015000200000000940000009000000000000100000", "NV after torn commit");
    require(response_size >= 32 && !memcmp(response + 16, expected, 16), "persistent data recovered");
    close_tpm();
    state = fopen(argv[2], "r+b");
    require(state != NULL, "open fixture to simulate corruption");
    require(fputc(0, state) == 0 && fseek(state, 64+16384, SEEK_SET) == 0 && fputc(0, state) == 0 && fclose(state) == 0, "corrupt both test generations");
    require(open_tpm(argv[2], 0) != 0 && open_tpm(argv[2], 1) != 0, "corrupt identity is never silently replaced");
    FreeLibrary(library);
    puts("PASS: TPM 2.0 crypto, entropy, persistence, exclusive locking, bounds, torn-write recovery and corruption refusal");
    return 0;
}
