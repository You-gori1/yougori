/* Minimal x64 UEFI ABI declarations, from the UEFI specification.
 * No host services, C runtime, disk writes, or external libraries are used. */
#pragma once
#include <stddef.h>
#include <stdint.h>

typedef uint64_t Status;
typedef void *Handle;
typedef uint16_t Char;
typedef struct { uint32_t a; uint16_t b, c; uint8_t d[8]; } Guid;
typedef struct { uint64_t signature; uint32_t revision, size, crc, reserved; } Header;
#define ERROR(s) (((Status)(s) >> 63) != 0)
#define NOT_FOUND ((Status)0x800000000000000eULL)

typedef struct File File;
struct File {
    uint64_t revision;
    Status (*open)(File *, File **, const Char *, uint64_t, uint64_t);
    Status (*close)(File *);
    void *delete_file;
    Status (*read)(File *, size_t *, void *);
};
typedef struct FileSystem FileSystem;
struct FileSystem { uint64_t revision; Status (*open_volume)(FileSystem *, File **); };
typedef struct {
    uint32_t revision;
    Handle parent;
    void *system;
    Handle device;
} LoadedImage;
typedef struct Text Text;
struct Text { void *reset; Status (*output)(Text *, const Char *); };
typedef struct {
    Header header;
    void *raise_tpl, *restore_tpl, *allocate_pages, *free_pages, *get_memory_map;
    Status (*allocate_pool)(uint32_t, size_t, void **);
    Status (*free_pool)(void *);
    void *create_event, *set_timer, *wait_for_event, *signal_event, *close_event, *check_event;
    void *install_protocol, *reinstall_protocol, *uninstall_protocol;
    Status (*handle_protocol)(Handle, Guid *, void **);
    void *reserved, *register_protocol, *locate_handle, *locate_device_path, *install_table;
    Status (*load_image)(uint8_t, Handle, void *, void *, size_t, Handle *);
    Status (*start_image)(Handle, size_t *, Char **);
    void *exit;
    Status (*unload_image)(Handle);
    void *exit_boot_services, *get_monotonic_count;
    Status (*stall)(size_t);
    Status (*set_watchdog)(size_t, uint64_t, size_t, Char *);
    Status (*connect_controller)(Handle, Handle *, void *, uint8_t);
    void *disconnect_controller, *open_protocol, *close_protocol;
    void *open_protocol_info, *protocols_per_handle;
    Status (*locate_handle_buffer)(uint32_t, Guid *, void *, size_t *, Handle **);
} BootServices;
typedef struct {
    Header header;
    Char *vendor;
    uint32_t firmware_revision;
    Handle console_in;
    void *input;
    Handle console_out;
    Text *output;
    Handle console_error;
    Text *error;
    void *runtime;
    BootServices *boot;
} SystemTable;

_Static_assert(sizeof(void *) == 8, "x64 UEFI only");
_Static_assert(offsetof(BootServices, load_image) == 200, "UEFI LoadImage ABI");
_Static_assert(offsetof(BootServices, locate_handle_buffer) == 312, "UEFI LocateHandleBuffer ABI");
_Static_assert(offsetof(SystemTable, boot) == 96, "UEFI system table ABI");
_Static_assert(offsetof(LoadedImage, device) == 24, "UEFI image ABI");
