/* Disposable guest-only regression fixture. Never included in production boot
 * media. Three guest-requested restarts preserve a file on the disposable disk,
 * then a guest-requested shutdown must power off rather than reboot forever. */
#include "uefi.h"
#include <intrin.h>
#pragma intrinsic(__outbyte)
#pragma intrinsic(__outword)
#pragma intrinsic(__rdtsc)
#pragma intrinsic(__halt)

typedef struct {
    Header header;
    void *get_time, *set_time, *get_wakeup_time, *set_wakeup_time;
    void *set_virtual_address_map, *convert_pointer;
    Status (*get_variable)(const Char *, Guid *, uint32_t *, size_t *, void *);
    void *get_next_variable_name;
    Status (*set_variable)(const Char *, Guid *, uint32_t, size_t, void *);
    void *get_next_high_monotonic_count;
    void (*reset_system)(uint32_t, Status, size_t, void *);
} RuntimeServices;
_Static_assert(offsetof(RuntimeServices, get_variable) == 72, "UEFI GetVariable ABI");
_Static_assert(offsetof(RuntimeServices, reset_system) == 104, "UEFI ResetSystem ABI");

static void trace(const char *text) {
    while (*text) __outbyte(0xe9, (unsigned char)*text++);
}

typedef struct {
    File base;
    Status (*write)(File *, size_t *, void *);
    void *get_position;
    Status (*set_position)(File *, uint64_t);
    void *get_info, *set_info;
    Status (*flush)(File *);
} WritableFile;
_Static_assert(offsetof(WritableFile, write) == 40, "UEFI File Write ABI");

static void fail(const char *message) { trace(message); for (;;) __halt(); }

Status efi_main(Handle image, SystemTable *system) {
    (void)image;
    system->boot->set_watchdog(0, 0, 0, 0);
    Guid fs_guid = {0x964e5b22, 0x6459, 0x11d2, {0x8e,0x39,0,0xa0,0xc9,0x69,0x72,0x3b}};
    Guid image_guid = {0x5b1b31a1,0x9562,0x11d2,{0x8e,0x3f,0,0xa0,0xc9,0x69,0x72,0x3b}};
    LoadedImage *loaded = 0;
    FileSystem *fs = 0;
    File *root = 0, *counter = 0;
    if (ERROR(system->boot->handle_protocol(image, &image_guid, (void **)&loaded)) ||
        ERROR(system->boot->handle_protocol(loaded->device, &fs_guid, (void **)&fs)) ||
        ERROR(fs->open_volume(fs, &root)) ||
        ERROR(root->open(root, &counter, L"\\restart-count", 0x8000000000000003ULL, 0))) fail("RESTART_TEST_OPEN_ERROR\n");
    uint32_t count = 0;
    size_t size = sizeof(count);
    if (ERROR(counter->read(counter, &size, &count))) fail("RESTART_TEST_READ_ERROR\n");
    trace("RESTART_TEST_BOOT_"); __outbyte(0xe9, (unsigned char)('0' + count)); trace("\n");
    if (count < 3) {
        ++count;
        WritableFile *file = (WritableFile *)counter;
        size = sizeof(count);
        if (ERROR(file->set_position(counter, 0)) || ERROR(file->write(counter, &size, &count)) ||
            size != sizeof(count) || ERROR(file->flush(counter))) fail("RESTART_TEST_SAVE_ERROR\n");
        counter->close(counter); root->close(root);
        /* This pinned OVMF build locates SNP secrets at 0x80d000, with
         * SvsmSize at +0x148. On our ordinary (non-SNP) disposable guest it
         * is just RAM. Reproduce Windows leaving it nonzero across reset:
         * the firmware must not infer SVSM support from stale RAM. */
        *(volatile uint64_t *)(uintptr_t)0x80d148 = 1;
        trace("RESTART_TEST_STALE_SNP_RAM\n");
        trace("RESTART_TEST_RESET_REQUEST\n");
        /* Exercise the actual UEFI service, including firmware callbacks,
         * rather than bypassing that path with direct chipset writes. */
        ((RuntimeServices *)system->runtime)->reset_system(count == 2 ? 1 : 0, 0, 0, 0);
    } else {
        counter->close(counter); root->close(root);
        trace("RESTART_TEST_COMPLETE\n");
        uint64_t started = __rdtsc();
        while (__rdtsc() - started < 5000000000ULL) { _mm_pause(); }
        __outword(0x604, 0x2000); /* Q35 PMBASE + PM1_CNT, ACPI S5 shutdown */
    }
    /* Port writes return before QEMU services the pending reset/shutdown. */
    for (;;) __halt();
}
