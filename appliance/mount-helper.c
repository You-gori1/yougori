#define _GNU_SOURCE

#include <errno.h>
#include <fcntl.h>
#include <sched.h>
#include <stdint.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/mount.h>
#include <sys/syscall.h>
#include <unistd.h>

#ifndef OPEN_TREE_CLONE
#define OPEN_TREE_CLONE 1
#endif
#ifndef OPEN_TREE_CLOEXEC
#define OPEN_TREE_CLOEXEC O_CLOEXEC
#endif
#ifndef MOVE_MOUNT_F_EMPTY_PATH
#define MOVE_MOUNT_F_EMPTY_PATH 0x00000004
#endif
#ifndef MOVE_MOUNT_T_EMPTY_PATH
#define MOVE_MOUNT_T_EMPTY_PATH 0x00000040
#endif
#ifndef AT_EMPTY_PATH
#define AT_EMPTY_PATH 0x1000
#endif
#ifndef MOUNT_ATTR_RDONLY
#define MOUNT_ATTR_RDONLY 0x00000001
#endif
#ifndef SYS_open_tree
#define SYS_open_tree 428
#endif
#ifndef SYS_move_mount
#define SYS_move_mount 429
#endif
#ifndef SYS_mount_setattr
#define SYS_mount_setattr 442
#endif

struct mount_attr {
  uint64_t attr_set;
  uint64_t attr_clr;
  uint64_t propagation;
  uint64_t userns_fd;
};

static void fail(const char *operation) {
  fprintf(stderr, "%s: %s\n", operation, strerror(errno));
  exit(1);
}

static bool starts_with(const char *value, const char *prefix) {
  return strncmp(value, prefix, strlen(prefix)) == 0;
}

static void make_directories(char *path) {
  for (char *cursor = path + 1; *cursor != '\0'; ++cursor) {
    if (*cursor != '/') {
      continue;
    }
    *cursor = '\0';
    if (mkdir(path, 0755) != 0 && errno != EEXIST) {
      fail("create mount target parent");
    }
    *cursor = '/';
  }
  if (mkdir(path, 0755) != 0 && errno != EEXIST) {
    fail("create mount target");
  }
}

int main(int argc, char **argv) {
  bool unmount_only = argc == 4 && strcmp(argv[2], "--unmount") == 0;
  if (argc != 5 && !unmount_only) {
    fprintf(stderr, "usage: opendock-mount-helper <pid> <source> <destination> <read-only>\n");
    return 2;
  }
  char *end = NULL;
  long pid = strtol(argv[1], &end, 10);
  if (pid <= 0 || end == argv[1] || *end != '\0') {
    fprintf(stderr, "invalid target process identifier\n");
    return 2;
  }
  const char *source = argv[2];
  const char *destination = argv[3];
  bool read_only = !unmount_only && strcmp(argv[4], "true") == 0;
  if ((!unmount_only && !starts_with(source, "/var/lib/opendock/shares/") &&
       !starts_with(source, "/var/lib/opendock/secrets/")) ||
      (!starts_with(destination, "/opendock/shared/") &&
       !starts_with(destination, "/opendock/secrets/")) ||
      strstr(source, "..") != NULL || strstr(destination, "..") != NULL) {
    fprintf(stderr, "mount path is outside the Yougori share roots\n");
    return 2;
  }

  char target[4096];
  char namespace_path[128];
  if (snprintf(target, sizeof(target), "/proc/%ld/root%s", pid, destination) >=
          (int)sizeof(target) ||
      snprintf(namespace_path, sizeof(namespace_path), "/proc/%ld/ns/mnt", pid) >=
          (int)sizeof(namespace_path)) {
    fprintf(stderr, "mount target path is too long\n");
    return 2;
  }
  if (unmount_only) {
    // Do not require an umount binary in the container, and do not chroot into
    // an untrusted image. Resolve its mount before entering its namespace.
    int namespace_fd = open(namespace_path, O_RDONLY | O_CLOEXEC);
    if (namespace_fd < 0 && errno == ENOENT) return 0; // exited container
    if (namespace_fd < 0) fail("open environment mount namespace");
    int target_fd = open(target, O_PATH | O_DIRECTORY | O_CLOEXEC | O_NOFOLLOW);
    if (target_fd < 0 && errno == ENOENT) { close(namespace_fd); return 0; }
    if (target_fd < 0) fail("open unmount target");
    if (setns(namespace_fd, CLONE_NEWNS) != 0) fail("enter environment mount namespace");
    if (fchdir(target_fd) != 0) fail("enter unmount target");
    if (umount2(".", MNT_DETACH) != 0 && errno != EINVAL && errno != ENOENT) fail("detach shared folder");
    close(target_fd);
    close(namespace_fd);
    return 0;
  }
  make_directories(target);

  int tree = syscall(SYS_open_tree, AT_FDCWD, source,
                     OPEN_TREE_CLONE | OPEN_TREE_CLOEXEC);
  if (tree < 0) {
    fail("clone source mount");
  }
  if (read_only) {
    struct mount_attr attributes = {.attr_set = MOUNT_ATTR_RDONLY};
    if (syscall(SYS_mount_setattr, tree, "", AT_EMPTY_PATH, &attributes,
                sizeof(attributes)) != 0) {
      fail("make cloned mount read-only");
    }
  }
  int target_fd = open(target, O_PATH | O_DIRECTORY | O_CLOEXEC | O_NOFOLLOW);
  if (target_fd < 0) {
    fail("open mount target");
  }
  int namespace_fd = open(namespace_path, O_RDONLY | O_CLOEXEC);
  if (namespace_fd < 0) {
    fail("open environment mount namespace");
  }
  if (setns(namespace_fd, CLONE_NEWNS) != 0) {
    fail("enter environment mount namespace");
  }
  if (syscall(SYS_move_mount, tree, "", target_fd, "",
              MOVE_MOUNT_F_EMPTY_PATH | MOVE_MOUNT_T_EMPTY_PATH) != 0) {
    fail("attach cloned mount");
  }
  close(namespace_fd);
  close(target_fd);
  close(tree);
  return 0;
}
